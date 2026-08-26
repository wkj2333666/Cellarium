use super::kitty_terminal::{KittyStreamParser, consume_shared_frame};
use std::collections::HashSet;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const INPUT_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_WARMUP: Duration = Duration::from_secs(1);
const FRAME_WINDOW: Duration = Duration::from_secs(3);
const MAX_READS_PER_PUMP: usize = 16;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct TerminalProbeReport {
    pub host: String,
    pub terminal_columns: u16,
    pub terminal_rows: u16,
    pub observed_frames: usize,
    pub kitty_frame_hz: f64,
    pub pause_ack_latency_ms: f64,
    pub pause_text_latency_ms: f64,
    pub pause_frame_latency_ms: f64,
    pub clear_ack_latency_ms: f64,
    pub clear_frame_latency_ms: f64,
    pub mouse_ack_latency_ms: f64,
    pub mouse_frame_latency_ms: f64,
    pub frame_intervals_ms: Vec<f64>,
    pub frame_sizes: Vec<usize>,
    pub workbench_apply_latency_ms: f64,
    pub workbench_authoritative_clean: bool,
}

#[derive(Clone, Copy)]
struct FrameObservation {
    at: Instant,
    hash: u64,
    size: usize,
}

struct CapturedGraphics {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct PtySession {
    master: i32,
    child: Child,
    parser: KittyStreamParser,
    output: Vec<u8>,
    trace_output: Arc<Mutex<Vec<u8>>>,
    trace_thread: Option<JoinHandle<()>>,
    screen: TerminalScreen,
    frames: Vec<FrameObservation>,
    active_kitty_images: HashSet<u32>,
    latest_graphics: Option<CapturedGraphics>,
    workspace_root: std::path::PathBuf,
}

enum EscapeState {
    Ground,
    Escape,
    Csi(Vec<u8>),
    String { saw_escape: bool },
}

pub struct TerminalScreen {
    width: usize,
    height: usize,
    column: usize,
    row: usize,
    cells: Vec<u8>,
    styles: Vec<u64>,
    current_style: u64,
    state: EscapeState,
    utf8_remaining: u8,
    utf8_hash: u8,
}

impl TerminalScreen {
    pub fn new(width: u16, height: u16) -> Self {
        let width = width as usize;
        let height = height as usize;
        Self {
            width,
            height,
            column: 0,
            row: 0,
            cells: vec![b' '; width * height],
            styles: vec![0; width * height],
            current_style: 0,
            state: EscapeState::Ground,
            utf8_remaining: 0,
            utf8_hash: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match &mut self.state {
                EscapeState::Ground => match byte {
                    0x1b => self.state = EscapeState::Escape,
                    b'\r' => self.column = 0,
                    b'\n' => self.row = (self.row + 1).min(self.height.saturating_sub(1)),
                    0x08 => self.column = self.column.saturating_sub(1),
                    0x20..=0x7e => self.put(byte),
                    0xc0..=0xf7 => {
                        self.utf8_hash = byte.wrapping_mul(31);
                        self.utf8_remaining = if byte < 0xe0 {
                            1
                        } else if byte < 0xf0 {
                            2
                        } else {
                            3
                        };
                    }
                    0x80..=0xbf if self.utf8_remaining > 0 => {
                        self.utf8_hash = self.utf8_hash.wrapping_mul(31) ^ byte;
                        self.utf8_remaining -= 1;
                        if self.utf8_remaining == 0 {
                            self.put(0x80 | (self.utf8_hash & 0x7f));
                        }
                    }
                    _ => {}
                },
                EscapeState::Escape => match byte {
                    b'[' => self.state = EscapeState::Csi(Vec::new()),
                    b']' | b'P' | b'_' => self.state = EscapeState::String { saw_escape: false },
                    _ => self.state = EscapeState::Ground,
                },
                EscapeState::Csi(parameters) => {
                    if (0x40..=0x7e).contains(&byte) {
                        let parameters = std::mem::take(parameters);
                        self.apply_csi(&parameters, byte);
                        self.state = EscapeState::Ground;
                    } else {
                        parameters.push(byte);
                    }
                }
                EscapeState::String { saw_escape } => {
                    if (*saw_escape && byte == b'\\') || byte == 0x07 {
                        self.state = EscapeState::Ground;
                    } else {
                        *saw_escape = byte == 0x1b;
                    }
                }
            }
        }
    }

    pub fn contains(&self, needle: &[u8]) -> bool {
        self.cells
            .chunks(self.width)
            .any(|line| contains(line, needle))
    }

    pub fn dump(&self) -> String {
        self.cells
            .chunks(self.width)
            .map(|line| String::from_utf8_lossy(line).trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\\n")
    }

    pub fn visual_hash(&self, x: usize, y: usize, width: usize, height: usize) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for row in y..(y + height).min(self.height) {
            for column in x..(x + width).min(self.width) {
                let index = row * self.width + column;
                hash ^= u64::from(self.cells[index]);
                hash = hash.wrapping_mul(0x100000001b3);
                hash ^= self.styles[index];
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        hash
    }

    fn put(&mut self, byte: u8) {
        if self.column < self.width && self.row < self.height {
            let index = self.row * self.width + self.column;
            self.cells[index] = byte;
            self.styles[index] = self.current_style;
        }
        self.column = (self.column + 1).min(self.width.saturating_sub(1));
    }

    fn apply_csi(&mut self, bytes: &[u8], command: u8) {
        let values = bytes
            .split(|byte| *byte == b';')
            .map(|value| {
                value
                    .iter()
                    .copied()
                    .filter(u8::is_ascii_digit)
                    .fold(0_usize, |number, digit| {
                        number * 10 + usize::from(digit - b'0')
                    })
            })
            .collect::<Vec<_>>();
        let value = |index: usize, default: usize| {
            values
                .get(index)
                .copied()
                .filter(|value| *value > 0)
                .unwrap_or(default)
        };
        match command {
            b'H' | b'f' => {
                self.row = value(0, 1).saturating_sub(1).min(self.height - 1);
                self.column = value(1, 1).saturating_sub(1).min(self.width - 1);
            }
            b'G' => self.column = value(0, 1).saturating_sub(1).min(self.width - 1),
            b'A' => self.row = self.row.saturating_sub(value(0, 1)),
            b'B' => self.row = (self.row + value(0, 1)).min(self.height - 1),
            b'C' => self.column = (self.column + value(0, 1)).min(self.width - 1),
            b'D' => self.column = self.column.saturating_sub(value(0, 1)),
            b'J' if values.first().copied() == Some(2) => {
                self.cells.fill(b' ');
                self.styles.fill(0);
            }
            b'K' => {
                let mode = values.first().copied().unwrap_or(0);
                let start = self.row * self.width;
                let range = match mode {
                    1 => start..start + self.column + 1,
                    2 => start..start + self.width,
                    _ => start + self.column..start + self.width,
                };
                self.cells[range.clone()].fill(b' ');
                self.styles[range].fill(0);
            }
            b'm' => {
                if bytes.is_empty() || values.iter().all(|value| *value == 0) {
                    self.current_style = 0;
                } else {
                    self.current_style = hash_bytes(bytes);
                }
            }
            _ => {}
        }
    }
}

impl PtySession {
    fn spawn(host: &str, columns: u16, rows: u16) -> io::Result<Self> {
        Self::spawn_with_graphics(host, columns, rows, true)
    }

    fn spawn_with_graphics(
        host: &str,
        columns: u16,
        rows: u16,
        graphics: bool,
    ) -> io::Result<Self> {
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::grantpt(master) } != 0 || unsafe { libc::unlockpt(master) } != 0 {
            unsafe { libc::close(master) };
            return Err(io::Error::last_os_error());
        }
        let mut peer_name = [0 as libc::c_char; 128];
        if unsafe { libc::ptsname_r(master, peer_name.as_mut_ptr(), peer_name.len()) } != 0 {
            unsafe { libc::close(master) };
            return Err(io::Error::last_os_error());
        }
        let slave = unsafe { libc::open(peer_name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if slave < 0 {
            unsafe { libc::close(master) };
            return Err(io::Error::last_os_error());
        }
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(slave, libc::TIOCSWINSZ, &winsize) } != 0 {
            unsafe {
                libc::close(slave);
                libc::close(master);
            }
            return Err(io::Error::last_os_error());
        }
        let flags = unsafe { libc::fcntl(master, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0
        {
            unsafe {
                libc::close(slave);
                libc::close(master);
            }
            return Err(io::Error::last_os_error());
        }

        let stdin = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
        let stdout = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
        let ssh_command = std::env::var("CELLARIUM_E2E_SSH_COMMAND").unwrap_or_else(|_| {
            std::env::var_os("CELLARIUM_E2E_SSH_CONFIG")
                .map(|path| format!("ssh -F {}", path.to_string_lossy()))
                .unwrap_or_else(|| "ssh".into())
        });
        let client = std::env::var_os("CELLARIUM_E2E_CLIENT")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_cellarium").into());
        let workspace_root = std::env::temp_dir().join(format!(
            "cellarium-terminal-probe-{}-{}",
            std::process::id(),
            NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&workspace_root)?;
        let mut command = Command::new(client);
        command
            .args(["connect", host])
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::piped())
            .env(
                "TERM",
                if graphics {
                    "xterm-kitty"
                } else {
                    "xterm-256color"
                },
            )
            .env("CELLARIUM_CELL_WIDTH", "8")
            .env("CELLARIUM_CELL_HEIGHT", "16")
            .env("CELLARIUM_E2E_TRACE", "1")
            .env("CELLARIUM_SSH_COMMAND", ssh_command)
            .env("XDG_DATA_HOME", &workspace_root)
            .env_remove("SSH_CONNECTION")
            .env_remove("SSH_TTY");
        if graphics {
            command.env("KITTY_WINDOW_ID", "1");
        } else {
            command.env_remove("KITTY_WINDOW_ID");
        }
        let mut child = command.spawn()?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("failed to capture E2E trace stream"))?;
        let trace_output = Arc::new(Mutex::new(Vec::new()));
        let trace_sink = Arc::clone(&trace_output);
        let trace_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                trace_sink
                    .lock()
                    .expect("trace buffer mutex poisoned")
                    .extend_from_slice(&buffer[..count]);
            }
        });
        unsafe { libc::close(slave) };
        Ok(Self {
            master,
            child,
            parser: KittyStreamParser::default(),
            output: Vec::new(),
            trace_output,
            trace_thread: Some(trace_thread),
            screen: TerminalScreen::new(columns, rows),
            frames: Vec::new(),
            active_kitty_images: HashSet::new(),
            latest_graphics: None,
            workspace_root,
        })
    }

    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let written = unsafe { libc::write(self.master, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        if written as usize != bytes.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short PTY write"));
        }
        Ok(())
    }

    /// Send a user-like SGR mouse click: press, wait for the semantic result,
    /// then release and let the release event drain before the next action.
    /// Keeping press and release in one PTY write can hide ordering bugs under
    /// a continuously redrawing half-block screen.
    fn click_until<F>(
        &mut self,
        description: &str,
        column: u16,
        row: u16,
        timeout: Duration,
        condition: F,
    ) -> io::Result<()>
    where
        F: Fn(&mut Self) -> bool,
    {
        self.write(format!("\x1b[<0;{column};{row}M").as_bytes())?;
        self.pump_until(description, timeout, condition)?;
        self.write(format!("\x1b[<0;{column};{row}m").as_bytes())?;
        self.pump_for(Duration::from_millis(50))
    }

    /// Drag along a short path at a human-like cadence so the test exercises
    /// the same event ordering as an actual terminal mouse instead of flooding
    /// the PTY input queue with hundreds of synthetic motion reports.
    fn drag_path(&mut self, button: u8, points: &[(u16, u16)]) -> io::Result<()> {
        let Some(&(first_column, first_row)) = points.first() else {
            return Ok(());
        };
        self.write(format!("\x1b[<{button};{first_column};{first_row}M").as_bytes())?;
        self.pump_for(Duration::from_millis(20))?;
        for &(column, row) in &points[1..] {
            self.write(format!("\x1b[<{};{column};{row}M", button + 32).as_bytes())?;
            self.pump_for(Duration::from_millis(10))?;
        }
        let &(last_column, last_row) = points.last().expect("non-empty drag path");
        self.write(format!("\x1b[<{button};{last_column};{last_row}m").as_bytes())?;
        self.pump_for(Duration::from_millis(50))
    }

    fn trace_len(&self) -> usize {
        self.trace_output
            .lock()
            .expect("trace buffer mutex poisoned")
            .len()
    }

    fn trace_contains_since(&self, start: usize, needle: &[u8]) -> bool {
        let trace = self
            .trace_output
            .lock()
            .expect("trace buffer mutex poisoned");
        start <= trace.len() && contains(&trace[start..], needle)
    }

    fn last_trace_line_since(&self, start: usize, prefix: &[u8]) -> Option<Vec<u8>> {
        let trace = self
            .trace_output
            .lock()
            .expect("trace buffer mutex poisoned");
        if start > trace.len() {
            return None;
        }
        trace[start..]
            .split(|byte| *byte == b'\n')
            .rev()
            .find(|line| contains(line, prefix))
            .map(|line| line.to_vec())
    }

    fn trace_tail(&self) -> String {
        let trace = self
            .trace_output
            .lock()
            .expect("trace buffer mutex poisoned");
        String::from_utf8_lossy(&trace[trace.len().saturating_sub(1000)..]).into_owned()
    }

    fn wait_for_stable_new_kitty_frame(
        &mut self,
        description: &str,
        checkpoint: usize,
        old_hash: Option<u64>,
    ) -> io::Result<u64> {
        let result = self.pump_until(description, INPUT_TIMEOUT, |session| {
            let fresh = &session.frames[checkpoint.min(session.frames.len())..];
            // Static Workbench scenes are deduplicated and intentionally emit
            // only one fresh graphics frame.  Treat that frame as stable once
            // it has remained the latest presentation for 120 ms; requiring
            // a duplicate frame would turn the optimization into a timeout.
            fresh.last().is_some_and(|last| {
                Some(last.hash) != old_hash && last.at.elapsed() >= Duration::from_millis(120)
            })
        });
        if let Err(error) = result {
            return Err(io::Error::new(
                error.kind(),
                format!("{error}; trace={}", self.trace_tail()),
            ));
        }
        Ok(self.frames.last().expect("stable Kitty frame").hash)
    }

    fn wait_for_stable_new_terminal_visual(
        &mut self,
        description: &str,
        region: (usize, usize, usize, usize),
        old_hash: Option<u64>,
    ) -> io::Result<u64> {
        let deadline = Instant::now() + INPUT_TIMEOUT;
        let mut candidate = None::<(u64, Instant)>;
        loop {
            self.pump_once()?;
            let hash = self
                .screen
                .visual_hash(region.0, region.1, region.2, region.3);
            if Some(hash) != old_hash {
                match candidate {
                    Some((previous, since)) if previous == hash => {
                        if since.elapsed() >= Duration::from_millis(120) {
                            return Ok(hash);
                        }
                    }
                    _ => candidate = Some((hash, Instant::now())),
                }
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(io::Error::other(format!(
                    "cellarium exited with {status} while waiting for {description}; trace: {}",
                    self.trace_tail()
                )));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for stable visual change: {description}; trace={}; screen=\\n{}",
                        self.trace_tail(),
                        self.screen.dump()
                    ),
                ));
            }
        }
    }

    fn pump_once(&mut self) -> io::Result<()> {
        let mut poll = libc::pollfd {
            fd: self.master,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 20) };
        if ready < 0 {
            return Err(io::Error::last_os_error());
        }
        if ready == 0 || poll.revents & libc::POLLIN == 0 {
            return Ok(());
        }
        for _ in 0..MAX_READS_PER_PUMP {
            let mut buffer = [0_u8; 65_536];
            let count =
                unsafe { libc::read(self.master, buffer.as_mut_ptr().cast(), buffer.len()) };
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(libc::EIO)
                {
                    return Ok(());
                }
                return Err(error);
            }
            if count == 0 {
                return Ok(());
            }
            let bytes = &buffer[..count as usize];
            self.output.extend_from_slice(bytes);
            self.screen.push(bytes);
            for command in self.parser.push(bytes) {
                observe_kitty_placement(&command.control, &mut self.active_kitty_images);
                if command.control.split(',').any(|field| field == "t=s") {
                    let dimensions = kitty_dimensions(&command.control);
                    let frame = consume_shared_frame(&command)?;
                    self.frames.push(FrameObservation {
                        at: Instant::now(),
                        hash: hash_bytes(&frame.bytes),
                        size: frame.bytes.len(),
                    });
                    if let Some((width, height)) = dimensions
                        && let Some(rgba) =
                            decode_kitty_pixels(&command.control, width, height, &frame.bytes)
                    {
                        self.latest_graphics = Some(CapturedGraphics {
                            width,
                            height,
                            rgba,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn pump_until(
        &mut self,
        description: &str,
        timeout: Duration,
        predicate: impl Fn(&mut Self) -> bool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump_once()?;
            if predicate(self) {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(io::Error::other(format!(
                    "cellarium exited with {status} while waiting for {description}: {}; trace: {}",
                    String::from_utf8_lossy(&self.output),
                    self.trace_tail()
                )));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for {description}; frames={}, output_bytes={}; tail={:?}; screen=\\n{}; trace={}",
                        self.frames.len(),
                        self.output.len(),
                        String::from_utf8_lossy(
                            &self.output[self.output.len().saturating_sub(500)..]
                        ),
                        self.screen.dump(),
                        self.trace_tail()
                    ),
                ));
            }
        }
    }

    fn pump_for(&mut self, duration: Duration) -> io::Result<()> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.pump_once()?;
            if let Some(status) = self.child.try_wait()? {
                return Err(io::Error::other(format!("cellarium exited with {status}")));
            }
        }
        Ok(())
    }

    fn wait_for_successful_exit(&mut self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump_once()?;
            if let Some(status) = self.child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "cellarium exited unsuccessfully: {status}"
                    )))
                };
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for clean client exit",
                ));
            }
        }
    }

    fn capture_state(&self, label: &str) -> io::Result<()> {
        let Some(directory) = std::env::var_os("CELLARIUM_E2E_CAPTURE_DIR") else {
            return Ok(());
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join(format!("{label}.txt")), self.screen.dump())?;
        if !self.active_kitty_images.is_empty()
            && let Some(graphics) = &self.latest_graphics
        {
            image::save_buffer_with_format(
                directory.join(format!("{label}.png")),
                &graphics.rgba,
                graphics.width,
                graphics.height,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .map_err(io::Error::other)?;
        }
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.trace_thread.take() {
            let _ = thread.join();
        }
        unsafe { libc::close(self.master) };
        let _ = std::fs::remove_dir_all(&self.workspace_root);
    }
}

pub fn run_terminal_probe(host: &str) -> io::Result<TerminalProbeReport> {
    let columns = 64;
    let rows = 20;
    let mut session = PtySession::spawn(host, columns, rows)?;
    session.pump_until("first consumed Kitty frame", STARTUP_TIMEOUT, |session| {
        !session.frames.is_empty()
    })?;

    // The first graphics frame only proves that the Kitty path is alive. It
    // may precede server startup/snapshot synchronisation by hundreds of
    // milliseconds, so including the next interval reports startup latency as
    // steady-state render cadence. Warm the complete C/S pipeline first, then
    // measure every interval in a separate uninterrupted window.
    session.pump_for(FRAME_WARMUP)?;
    let cadence_start = session.frames.len() - 1;
    let cadence_started_at = Instant::now();
    session.pump_for(FRAME_WINDOW)?;
    let cadence_elapsed = cadence_started_at.elapsed().as_secs_f64();
    let cadence_frames = &session.frames[cadence_start..];
    if cadence_frames.len() < 2 {
        return Err(io::Error::other("fewer than two Kitty frames consumed"));
    }
    // Count frames against the complete wall-clock window. This deliberately
    // includes any empty tail after the final frame, so a renderer that runs
    // briefly and then stalls cannot report an inflated cadence.
    let kitty_frame_hz = (cadence_frames.len() - 1) as f64 / cadence_elapsed;
    let cadence_intervals_ms = cadence_frames
        .windows(2)
        .map(|pair| pair[1].at.duration_since(pair[0].at).as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();

    let pause_started = Instant::now();
    session.write(b" ")?;
    session.pump_until("server pause acknowledgement", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"ack 1") && session.screen.contains(b"paused")
    })?;
    let pause_ack_at = Instant::now();
    let frames_after_pause_ack = session.frames.len();
    session.pump_for(Duration::from_millis(300))?;
    let paused_hash = session
        .frames
        .last()
        .map(|frame| frame.hash)
        .ok_or_else(|| io::Error::other("pause removed the displayed Kitty frame"))?;
    assert!(
        session.frames[frames_after_pause_ack..]
            .iter()
            .all(|frame| frame.hash == paused_hash),
        "paused output must remain stable"
    );
    let pause_frame = session.frames[frames_after_pause_ack..]
        .first()
        .copied()
        .unwrap_or_else(|| *session.frames.last().expect("pause frame"));
    let pause_ack_latency_ms = pause_ack_at.duration_since(pause_started).as_secs_f64() * 1_000.0;
    let pause_text_latency_ms = pause_ack_latency_ms;
    let pause_frame_latency_ms =
        pause_frame.at.duration_since(pause_started).as_secs_f64() * 1_000.0;

    let clear_started = Instant::now();
    session.write(b"c")?;
    session.pump_until("server clear acknowledgement", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"ack 2")
    })?;
    let clear_ack_at = Instant::now();
    let frames_after_clear_ack = session.frames.len();
    session.pump_until("cleared Kitty frame", INPUT_TIMEOUT, |session| {
        session.frames[frames_after_clear_ack..]
            .iter()
            .any(|frame| frame.hash != paused_hash)
    })?;
    let clear_frame = session.frames[frames_after_clear_ack..]
        .iter()
        .find(|frame| frame.hash != paused_hash)
        .copied()
        .expect("cleared frame");
    let cleared_hash = clear_frame.hash;
    session.pump_for(Duration::from_millis(300))?;
    assert!(
        session.frames[frames_after_clear_ack..]
            .iter()
            .all(|frame| frame.hash == cleared_hash),
        "cleared output must remain stable"
    );
    let clear_ack_latency_ms = clear_ack_at.duration_since(clear_started).as_secs_f64() * 1_000.0;
    let clear_frame_latency_ms =
        clear_frame.at.duration_since(clear_started).as_secs_f64() * 1_000.0;

    let mouse_started = Instant::now();
    let frames_before_mouse = session.frames.len();
    // A real click begins with a button-down event. A standalone motion event
    // can be acknowledged without establishing a paint stroke in some
    // terminal/input stacks and therefore is not a valid user-level probe.
    session.write(b"\x1b[<0;16;8M")?;
    session.pump_until("server mouse acknowledgement", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"ack 3")
    })?;
    let mouse_ack_at = Instant::now();
    for column in 17..=24 {
        session.write(format!("\x1b[<32;{column};8M").as_bytes())?;
        session.pump_for(Duration::from_millis(10))?;
    }
    session.write(b"\x1b[<0;24;8m")?;
    session.pump_for(Duration::from_millis(50))?;
    session.pump_until("mouse-edited Kitty frame", INPUT_TIMEOUT, |session| {
        session.frames[frames_before_mouse..]
            .iter()
            .any(|frame| frame.hash != cleared_hash)
    })?;
    let mouse_frame = session.frames[frames_before_mouse..]
        .iter()
        .find(|frame| frame.hash != cleared_hash)
        .copied()
        .expect("mouse-edited frame");
    let mouse_frame_latency_ms =
        mouse_frame.at.duration_since(mouse_started).as_secs_f64() * 1_000.0;
    let mouse_ack_latency_ms = mouse_ack_at.duration_since(mouse_started).as_secs_f64() * 1_000.0;

    let observed_frames = session.frames.len();
    let frame_intervals_ms = cadence_intervals_ms;
    let frame_sizes = session.frames.iter().map(|frame| frame.size).collect();
    session.write(b"q")?;
    session.wait_for_successful_exit(INPUT_TIMEOUT)?;

    Ok(TerminalProbeReport {
        host: host.to_string(),
        terminal_columns: columns,
        terminal_rows: rows,
        observed_frames,
        kitty_frame_hz,
        pause_ack_latency_ms,
        pause_text_latency_ms,
        pause_frame_latency_ms,
        clear_ack_latency_ms,
        clear_frame_latency_ms,
        mouse_ack_latency_ms,
        mouse_frame_latency_ms,
        frame_intervals_ms,
        frame_sizes,
        workbench_apply_latency_ms: 0.0,
        workbench_authoritative_clean: false,
    })
}

/// Drive the actual Kitty Workbench end to end: click/drag the canvas, operate
/// every editor section, type Growth source, and Apply the draft.
pub fn run_workbench_probe(host: &str) -> io::Result<f64> {
    // Use a wide, realistic Workbench terminal so the Inspector/editor pane
    // is present and user-visible while typing Growth source.
    let mut session = PtySession::spawn_with_graphics(host, 160, 40, true)?;
    session.pump_until("Workbench startup", STARTUP_TIMEOUT, |session| {
        session.screen.contains(b"Cellarium") && session.latest_graphics.is_some()
    })?;
    let startup = session
        .latest_graphics
        .as_ref()
        .ok_or_else(|| io::Error::other("startup simulation did not render graphics"))?;
    let bright = startup
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, pixel)| pixel[0].max(pixel[1]).max(pixel[2]) > 80)
        .map(|(index, _)| {
            (
                (index % startup.width as usize) as u32,
                (index / startup.width as usize) as u32,
            )
        })
        .collect::<Vec<_>>();
    let bright_bounds =
        bright
            .iter()
            .fold(None, |bounds: Option<(u32, u32, u32, u32)>, &(x, y)| {
                Some(match bounds {
                    None => (x, y, x, y),
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                })
            });
    let Some((min_x, min_y, max_x, max_y)) = bright_bounds else {
        return Err(io::Error::other(
            "startup simulation frame has no visible state",
        ));
    };
    if max_x.saturating_sub(min_x) * 2 < startup.width
        || max_y.saturating_sub(min_y) * 2 < startup.height
    {
        return Err(io::Error::other(format!(
            "startup simulation is not auto-fit: bright bounds {min_x},{min_y}..{max_x},{max_y} in {}x{}",
            startup.width, startup.height
        )));
    }
    let workbench_trace_start = session.trace_len();
    let workbench_frame_start = session.frames.len();
    let simulation_hash = session.frames.last().map(|frame| frame.hash);
    session.write(b"w")?;
    session.pump_until("Workbench shell", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Workbench")
            && session.screen.contains(b"World")
            && !session.active_kitty_images.is_empty()
    })?;
    let world_hash = Some(session.wait_for_stable_new_kitty_frame(
        "stable Workbench World graphics",
        workbench_frame_start,
        simulation_hash,
    )?);
    // Paint a real stroke (press, drag, release). Leaving the button held
    // would make subsequent keyboard input look like a drag to a real terminal.
    let paint_trace_start = session.trace_len();
    let paint_frame_start = session.frames.len();
    let mut stroke = b"\x1b[<0;35;18M".to_vec();
    for column in (37..=71).step_by(2) {
        stroke.extend_from_slice(format!("\x1b[<32;{column};18M").as_bytes());
    }
    stroke.extend_from_slice(b"\x1b[<0;71;18m");
    session.write(&stroke)?;
    session.pump_until("Workbench mouse paint", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(paint_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
    })?;
    let painted_hash = Some(session.wait_for_stable_new_kitty_frame(
        "painted World graphics",
        paint_frame_start,
        world_hash,
    )?);
    // Pan like a Kitty user: middle-button press, drag, release.
    let pan_frame_start = session.frames.len();
    session.write(b"\x1b[<1;60;12M\x1b[<33;64;14M\x1b[<1;64;14m")?;
    session.wait_for_stable_new_kitty_frame(
        "middle-button canvas pan",
        pan_frame_start,
        painted_hash,
    )?;
    session.capture_state("01-world-painted-and-panned")?;
    // Click Channels in the left Experiment outline (SGR coordinates are
    // one-based; the section is the fourth terminal row).
    let channel_frame_start = session.frames.len();
    session.click_until("Channels section", 5, 4, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Channels")
            && session.screen.contains(b"Preview: running state")
            && !session.active_kitty_images.is_empty()
    })?;
    session.wait_for_stable_new_kitty_frame(
        "authoritative Channels preview",
        channel_frame_start,
        world_hash,
    )?;
    session.capture_state("02-channels")?;
    // Freeze removes kernels targeting the channel. Verify undo/redo, then
    // leave the draft restored so the following kernel editor is meaningful.
    session.write(b"a")?;
    session.pump_until("channel controls", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"channel_2")
    })?;
    // Click the second rendered Inspector row, rather than cycling it by key.
    session.click_until(
        "clickable second channel row",
        130,
        14,
        INPUT_TIMEOUT,
        |session| session.screen.contains(b"selected: channel_2"),
    )?;
    session.write(b"cvxf\x1a\x19\x1a")?;
    let frames_before_kernels = session.frames.len();
    let graphics_before_kernels = session.frames.last().map(|frame| frame.hash);
    session.click_until("Kernels section", 5, 5, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"selected Kernels") && !session.active_kitty_images.is_empty()
    })?;
    let kernel_hash = Some(session.wait_for_stable_new_kitty_frame(
        "stable Kernels graphics",
        frames_before_kernels,
        graphics_before_kernels,
    )?);
    let kernel_zoom_start = session.frames.len();
    session.write(b"\x1b[<64;50;18M\x1b[<64;50;18M")?;
    let zoomed_kernel_hash = Some(session.wait_for_stable_new_kitty_frame(
        "mouse-wheel-zoomed kernel graphics",
        kernel_zoom_start,
        kernel_hash,
    )?);
    let kernel_pan_start = session.frames.len();
    session.write(b"\x1b[<1;50;18M\x1b[<33;54;20M\x1b[<1;54;20m")?;
    let panned_kernel_hash = Some(session.wait_for_stable_new_kitty_frame(
        "middle-panned kernel graphics",
        kernel_pan_start,
        zoomed_kernel_hash,
    )?);
    // Paint an expression-defined kernel cell. The editor materializes the
    // evaluated kernel so the result becomes directly editable.
    let kernel_trace_start = session.trace_len();
    let kernel_frame_start = session.frames.len();
    // The preceding pan moves the fitted anchor from roughly (75,20) to
    // (79,22).  Mutate that visible cell instead of a hard-coded empty corner.
    session.write(b"\x1b[<0;79;22M\x1b[<32;81;22M\x1b[<0;81;22m")?;
    session.pump_until("kernel mouse mutation", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(kernel_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
    })?;
    session.wait_for_stable_new_kitty_frame(
        "mouse-edited kernel graphics",
        kernel_frame_start,
        panned_kernel_hash,
    )?;
    session.capture_state("03-kernel-mouse-edit")?;
    let frames_before_growth = session.frames.len();
    let graphics_before_growth = session.frames.last().map(|frame| frame.hash);
    session.click_until("Growth section", 5, 6, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"selected Growth") && !session.active_kitty_images.is_empty()
    })?;
    session.wait_for_stable_new_kitty_frame(
        "stable Growth graphics",
        frames_before_growth,
        graphics_before_growth,
    )?;
    session.write(b"e")?;
    session.pump_until("Growth editor", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Growth") && session.screen.contains(b"EDITING")
    })?;
    // Replace the whole source with an exact-equality program. Uniform-only
    // sampling renders this flat, so the orange threshold markers are a
    // semantic framebuffer gate rather than a generic frame-hash check.
    let growth_hash = session.frames.last().map(|frame| frame.hash);
    let growth_frame_start = session.frames.len();
    let growth_trace_start = session.trace_len();
    // Adding a channel materializes the legacy kernel as `k1`, so use the
    // signature shown by the editor rather than a stale `potential` name.
    session.write(b"\x01if k1 == 2/6 || k1 == 3/6 { 1 } else { 0 }")?;
    session.pump_until("typed Growth source", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"k1 == 3/6")
            && session
                .last_trace_line_since(growth_trace_start, b"E2E_GROWTH_VALID")
                .is_some_and(|line| {
                    contains(&line, b"E2E_GROWTH_VALID valid=true") && contains(&line, b"k1 == 3/6")
                })
    })?;
    session.wait_for_stable_new_kitty_frame(
        "live Growth plot update",
        growth_frame_start,
        growth_hash,
    )?;
    let exact_markers = session
        .latest_graphics
        .as_ref()
        .map(|graphics| {
            graphics
                .rgba
                .chunks_exact(4)
                .filter(|pixel| pixel[0] == 255 && pixel[1] == 190 && pixel[2] == 70)
                .count()
        })
        .unwrap_or(0);
    if exact_markers < 10 {
        return Err(io::Error::other(format!(
            "discontinuous Growth curve rendered without exact markers ({exact_markers})"
        )));
    }
    session.capture_state("04-growth-expression-edit")?;
    session.write(b"\x1b")?;
    session.pump_for(Duration::from_millis(200))?;
    session.click_until("Experiment section", 5, 7, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Experiment review") && session.active_kitty_images.is_empty()
    })?;
    if !session.active_kitty_images.is_empty() {
        return Err(io::Error::other(
            "text-only Experiment must clear the previous Kitty placement",
        ));
    }
    session.capture_state("05-experiment-cleared")?;
    let frames_before_tiling = session.frames.len();
    let graphics_before_tiling = session.frames.last().map(|frame| frame.hash);
    session.click_until("Tiling section", 5, 3, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Tiling")
    })?;
    session.write(b"p")?;
    session.pump_until("square tiling editor", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Tiling")
            && session.screen.contains(b"exact edge-to-edge tiling")
            && !session.active_kitty_images.is_empty()
    })?;
    let tiling_hash = Some(session.wait_for_stable_new_kitty_frame(
        "stable square tiling graphics",
        frames_before_tiling,
        graphics_before_tiling,
    )?);
    // Grab the square vertex at world (0, 0), drag it eight graphics pixels,
    // and release. Undo afterwards so the final Apply remains a valid tiling.
    let tiling_trace_start = session.trace_len();
    let tiling_frame_start = session.frames.len();
    // The wrapped toolbar plus context occupy three rows. Drag the fitted
    // world-origin vertex far enough to produce an unmistakable new frame.
    session.write(b"\x1b[<0;75;21M\x1b[<32;85;21M\x1b[<0;85;21m")?;
    session.pump_until("tiling vertex mutation", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(tiling_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
    })?;
    session.wait_for_stable_new_kitty_frame(
        "mouse-dragged tiling vertex",
        tiling_frame_start,
        tiling_hash,
    )?;
    session.capture_state("06-tiling-vertex-drag")?;
    session.write(b"\x1a")?;
    session.pump_for(Duration::from_millis(200))?;
    session.click_until("Experiment review", 5, 7, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Experiment review")
            && !session.screen.contains(b"Periodic tiling editor")
            && session.active_kitty_images.is_empty()
    })?;
    if !session.active_kitty_images.is_empty() {
        return Err(io::Error::other(
            "returning to Experiment must clear the Tiling Kitty placement",
        ));
    }
    if session.trace_contains_since(workbench_trace_start, b"E2E_MOUSE_FORWARDED") {
        return Err(io::Error::other(
            "Workbench mouse input mutated the authoritative server before Apply",
        ));
    }
    let experiment_click_trace = session.trace_len();
    session.write(b"\x1b[<0;75;20M")?;
    session.pump_until(
        "Experiment canvas click handled without Apply",
        INPUT_TIMEOUT,
        |session| {
            session.trace_contains_since(experiment_click_trace, b"E2E_WORKBENCH_MOUSE applied=")
        },
    )?;
    if session.trace_contains_since(experiment_click_trace, b"E2E_APPLY_SENT") {
        return Err(io::Error::other(
            "clicking the Experiment canvas must not submit the draft",
        ));
    }
    let started = Instant::now();
    // Lowercase `a` is an unambiguous Apply fallback in Experiment review for
    // PTYs that cannot carry Ctrl+Enter modifiers through SSH.
    session.write(b"a")?;
    session.pump_until("Apply dispatch", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(workbench_trace_start, b"E2E_APPLY_SENT")
    })?;
    session.pump_until(
        "authoritative ApplyAccepted",
        Duration::from_secs(20),
        |session| session.trace_contains_since(workbench_trace_start, b"E2E_APPLY_ACCEPTED"),
    )?;
    let latency = started.elapsed().as_secs_f64() * 1_000.0;
    session.write(b"q")?;
    session.wait_for_successful_exit(INPUT_TIMEOUT)?;
    Ok(latency)
}

/// Exercise the same editor entry, mouse, keyboard, source editing, and Apply
/// path without any terminal graphics protocol.
pub fn run_workbench_fallback_probe(host: &str) -> io::Result<()> {
    let mut session = PtySession::spawn_with_graphics(host, 160, 40, false)?;
    session.pump_until("fallback startup", STARTUP_TIMEOUT, |session| {
        session.screen.contains(b"Cellarium")
    })?;
    session.write(b" ")?;
    session.pump_until("fallback pause", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"paused")
    })?;
    let workbench_trace_start = session.trace_len();
    let canvas_region = (25, 1, 98, 36);
    let simulation_hash = session.screen.visual_hash(
        canvas_region.0,
        canvas_region.1,
        canvas_region.2,
        canvas_region.3,
    );
    session.write(b"w")?;
    session.pump_until("fallback Workbench", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Workbench") && session.screen.contains(b"World")
    })?;
    let _world_hash = session.wait_for_stable_new_terminal_visual(
        "stable fallback World canvas",
        canvas_region,
        Some(simulation_hash),
    )?;
    // First erase a broad stroke from the nonzero default state. This gives
    // half-block a deterministic visual baseline instead of painting 1 over
    // a cell that may already be saturated.
    let erase_trace_start = session.trace_len();
    let stroke_path = (35..=71)
        .step_by(2)
        .map(|column| (column, 20))
        .collect::<Vec<_>>();
    session.drag_path(2, &stroke_path)?;
    session.pump_until("fallback mouse erase", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(erase_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
            && session.screen.contains(b"Dirty")
    })?;
    // Erasing an already-empty initial field is legitimately invisible. Let
    // the accepted erase settle, then use its actual visual as the baseline.
    session.pump_for(Duration::from_millis(200))?;
    let mut canvas_hash = session.screen.visual_hash(
        canvas_region.0,
        canvas_region.1,
        canvas_region.2,
        canvas_region.3,
    );
    let paint_trace_start = session.trace_len();
    session.drag_path(0, &stroke_path)?;
    session.pump_until("fallback mouse paint", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(paint_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
            && session.screen.contains(b"Dirty")
    })?;
    let painted_hash = session.wait_for_stable_new_terminal_visual(
        "fallback World paint",
        canvas_region,
        Some(canvas_hash),
    )?;
    canvas_hash = painted_hash;
    session.drag_path(1, &[(60, 12), (62, 13), (64, 14)])?;
    let _panned_hash = session.wait_for_stable_new_terminal_visual(
        "fallback middle-button World pan",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.click_until("fallback Kernels", 5, 5, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"selected Kernels")
    })?;
    canvas_hash = session.wait_for_stable_new_terminal_visual(
        "stable fallback Kernel canvas",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.write(b"\x1b[<64;50;18M\x1b[<64;50;18M")?;
    let zoomed_kernel_hash = session.wait_for_stable_new_terminal_visual(
        "fallback mouse-wheel kernel zoom",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.write(b"\x1b[<1;50;18M\x1b[<33;54;20M\x1b[<1;54;20m")?;
    canvas_hash = session.wait_for_stable_new_terminal_visual(
        "fallback middle-button kernel pan",
        canvas_region,
        Some(zoomed_kernel_hash),
    )?;
    let kernel_trace_start = session.trace_len();
    session.write(b"\x1b[<0;79;22M\x1b[<32;81;22M\x1b[<0;81;22m")?;
    session.pump_until("fallback kernel mutation", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(kernel_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
    })?;
    let kernel_hash = session.wait_for_stable_new_terminal_visual(
        "fallback kernel mouse edit",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.click_until("fallback Growth", 5, 6, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"selected Growth")
    })?;
    canvas_hash = session.wait_for_stable_new_terminal_visual(
        "stable fallback Growth canvas",
        canvas_region,
        Some(kernel_hash),
    )?;
    let growth_trace_start = session.trace_len();
    session.write(b"e\x01if potential == 2/6 || potential == 3/6 { 1 } else { 0 }")?;
    session.pump_until("fallback Growth source typing", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Growth")
            && session.screen.contains(b"EDITING")
            && session.screen.contains(b"potential == 3/6")
            && session
                .last_trace_line_since(growth_trace_start, b"E2E_GROWTH_VALID")
                .is_some_and(|line| {
                    contains(&line, b"E2E_GROWTH_VALID valid=true")
                        && contains(&line, b"potential == 3/6")
                })
    })?;
    let growth_hash = session.wait_for_stable_new_terminal_visual(
        "valid fallback Growth curve",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.write(b"\x1b")?;
    session.pump_for(Duration::from_millis(200))?;
    session.click_until("fallback Tiling", 5, 3, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Tiling")
    })?;
    session.write(b"p")?;
    session.pump_until("fallback square Tiling", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"section: Tiling")
            && session.screen.contains(b"exact edge-to-edge tiling")
    })?;
    canvas_hash = session.wait_for_stable_new_terminal_visual(
        "stable fallback Tiling canvas",
        canvas_region,
        Some(growth_hash),
    )?;
    let tiling_trace_start = session.trace_len();
    session.write(b"\x1b[<0;75;21M\x1b[<32;85;21M\x1b[<0;85;21m")?;
    session.pump_until("fallback tiling mutation", INPUT_TIMEOUT, |session| {
        session.trace_contains_since(tiling_trace_start, b"E2E_WORKBENCH_MOUSE applied=true")
    })?;
    session.wait_for_stable_new_terminal_visual(
        "fallback tiling vertex drag",
        canvas_region,
        Some(canvas_hash),
    )?;
    session.write(b"\x1a")?;
    session.click_until("fallback Experiment", 5, 7, INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Experiment review")
    })?;
    if session.trace_contains_since(workbench_trace_start, b"E2E_MOUSE_FORWARDED") {
        return Err(io::Error::other(
            "fallback Workbench input mutated the server before Apply",
        ));
    }
    let experiment_click_trace = session.trace_len();
    session.write(b"\x1b[<0;75;20M")?;
    session.pump_until(
        "fallback Experiment canvas click handled without Apply",
        INPUT_TIMEOUT,
        |session| {
            session.trace_contains_since(experiment_click_trace, b"E2E_WORKBENCH_MOUSE applied=")
        },
    )?;
    if session.trace_contains_since(experiment_click_trace, b"E2E_APPLY_SENT") {
        return Err(io::Error::other(
            "fallback Experiment canvas click must not submit the draft",
        ));
    }
    session.write(b"a")?;
    session.pump_until(
        "fallback authoritative ApplyAccepted",
        Duration::from_secs(20),
        |session| session.trace_contains_since(workbench_trace_start, b"E2E_APPLY_ACCEPTED"),
    )?;
    session.write(b"q")?;
    session.wait_for_successful_exit(INPUT_TIMEOUT)
}

pub fn write_report(report: &TerminalProbeReport) -> io::Result<()> {
    let path = std::env::var_os("CELLARIUM_E2E_TERMINAL_REPORT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("target/e2e-tinker-terminal.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let intervals = report
        .frame_intervals_ms
        .iter()
        .map(|value| format!("{value:.6}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sizes = report
        .frame_sizes
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let host = report.host.replace('\\', "\\\\").replace('"', "\\\"");
    let json = format!(
        concat!(
            "{{\n",
            "  \"host\": \"{}\",\n",
            "  \"terminal_columns\": {},\n",
            "  \"terminal_rows\": {},\n",
            "  \"observed_frames\": {},\n",
            "  \"kitty_frame_hz\": {:.6},\n",
            "  \"pause_ack_latency_ms\": {:.6},\n",
            "  \"pause_text_latency_ms\": {:.6},\n",
            "  \"pause_frame_latency_ms\": {:.6},\n",
            "  \"clear_ack_latency_ms\": {:.6},\n",
            "  \"clear_frame_latency_ms\": {:.6},\n",
            "  \"mouse_ack_latency_ms\": {:.6},\n",
            "  \"mouse_frame_latency_ms\": {:.6},\n",
            "  \"frame_intervals_ms\": [{}],\n",
            "  \"frame_sizes\": [{}],\n",
            "  \"workbench_apply_latency_ms\": {:.6},\n",
            "  \"workbench_authoritative_clean\": {}\n",
            "}}\n"
        ),
        host,
        report.terminal_columns,
        report.terminal_rows,
        report.observed_frames,
        report.kitty_frame_hz,
        report.pause_ack_latency_ms,
        report.pause_text_latency_ms,
        report.pause_frame_latency_ms,
        report.clear_ack_latency_ms,
        report.clear_frame_latency_ms,
        report.mouse_ack_latency_ms,
        report.mouse_frame_latency_ms,
        intervals,
        sizes,
        report.workbench_apply_latency_ms,
        report.workbench_authoritative_clean,
    );
    std::fs::write(path, json)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn kitty_dimensions(control: &str) -> Option<(u32, u32)> {
    let fields = control
        .split(',')
        .filter_map(|field| field.split_once('='))
        .collect::<std::collections::HashMap<_, _>>();
    Some((
        fields.get("s")?.parse().ok()?,
        fields.get("v")?.parse().ok()?,
    ))
}

pub fn decode_kitty_pixels(
    control: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Option<Vec<u8>> {
    let format = control
        .split(',')
        .find_map(|field| field.strip_prefix("f="))
        .unwrap_or("32");
    let pixel_count = width as usize * height as usize;
    match format {
        "32" if pixels.len() == pixel_count * 4 => Some(pixels.to_vec()),
        "24" if pixels.len() == pixel_count * 3 => {
            let mut rgba = Vec::with_capacity(pixel_count * 4);
            for rgb in pixels.chunks(3) {
                rgba.extend_from_slice(rgb);
                rgba.push(255);
            }
            Some(rgba)
        }
        _ => None,
    }
}

fn observe_kitty_placement(control: &str, active: &mut HashSet<u32>) {
    let fields = control
        .split(',')
        .filter_map(|field| field.split_once('='))
        .collect::<std::collections::HashMap<_, _>>();
    match (fields.get("a"), fields.get("d")) {
        (Some(&"T"), _) => {
            if let Some(id) = fields.get("i").and_then(|value| value.parse().ok()) {
                active.insert(id);
            }
        }
        (Some(&"d"), Some(&"I")) => {
            if let Some(id) = fields.get("i").and_then(|value| value.parse().ok()) {
                active.remove(&id);
            }
        }
        (Some(&"d"), Some(&"A")) => active.clear(),
        _ => {}
    }
}

/// Drive Workbench with a real Kitty PTY and verify that leaving the graphics
/// canvas for the text-only Experiment section removes the terminal image
/// placement. Checking only the textual screen misses stale Kitty overlays.
pub fn run_workbench_graphics_clear_probe(host: &str) -> io::Result<()> {
    let mut session = PtySession::spawn(host, 96, 30)?;
    session.pump_until("first simulation Kitty frame", STARTUP_TIMEOUT, |session| {
        !session.frames.is_empty()
    })?;
    session.write(b"w")?;
    session.pump_until("Workbench canvas", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"Canvas") && session.screen.contains(b"World")
    })?;
    // Outline rows are World(1), Tiling(2), Channels(3), Kernels(4),
    // Growth(5), Experiment(6); coordinates are one-based in SGR mouse.
    session.write(b"\x1b[<0;8;7M\x1b[<0;8;7m")?;
    session.pump_for(Duration::from_millis(350))?;
    assert!(
        session.active_kitty_images.is_empty(),
        "switching to text-only Experiment must leave no active Kitty image placement"
    );
    session.write(b"q")?;
    session.wait_for_successful_exit(INPUT_TIMEOUT)
}
