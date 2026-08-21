use super::kitty_terminal::{KittyStreamParser, consume_shared_frame};
use std::io;
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const INPUT_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_WINDOW: Duration = Duration::from_secs(3);

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
}

#[derive(Clone, Copy)]
struct FrameObservation {
    at: Instant,
    hash: u64,
    size: usize,
}

struct PtySession {
    master: i32,
    child: Child,
    parser: KittyStreamParser,
    output: Vec<u8>,
    screen: TerminalScreen,
    frames: Vec<FrameObservation>,
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
    state: EscapeState,
    utf8_remaining: u8,
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
            state: EscapeState::Ground,
            utf8_remaining: 0,
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
                        self.utf8_remaining = if byte < 0xe0 {
                            1
                        } else if byte < 0xf0 {
                            2
                        } else {
                            3
                        };
                    }
                    0x80..=0xbf if self.utf8_remaining > 0 => {
                        self.utf8_remaining -= 1;
                        if self.utf8_remaining == 0 {
                            self.put(b'?');
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

    fn put(&mut self, byte: u8) {
        if self.column < self.width && self.row < self.height {
            self.cells[self.row * self.width + self.column] = byte;
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
            b'J' if values.first().copied() == Some(2) => self.cells.fill(b' '),
            b'K' => {
                let mode = values.first().copied().unwrap_or(0);
                let start = self.row * self.width;
                let range = match mode {
                    1 => start..start + self.column + 1,
                    2 => start..start + self.width,
                    _ => start + self.column..start + self.width,
                };
                self.cells[range].fill(b' ');
            }
            _ => {}
        }
    }
}

impl PtySession {
    fn spawn(host: &str, columns: u16, rows: u16) -> io::Result<Self> {
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
        let stderr = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
        let ssh_command = std::env::var_os("CELLARIUM_E2E_SSH_CONFIG")
            .map(|path| format!("ssh -F {}", path.to_string_lossy()))
            .unwrap_or_else(|| "ssh".into());
        let client = std::env::var_os("CELLARIUM_E2E_CLIENT")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_cellarium").into());
        let child = Command::new(client)
            .args(["connect", host])
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .env("TERM", "xterm-kitty")
            .env("KITTY_WINDOW_ID", "1")
            .env("CELLARIUM_CELL_WIDTH", "8")
            .env("CELLARIUM_CELL_HEIGHT", "16")
            .env("CELLARIUM_SSH_COMMAND", ssh_command)
            .env_remove("SSH_CONNECTION")
            .env_remove("SSH_TTY")
            .spawn()?;
        unsafe { libc::close(slave) };
        Ok(Self {
            master,
            child,
            parser: KittyStreamParser::default(),
            output: Vec::new(),
            screen: TerminalScreen::new(columns, rows),
            frames: Vec::new(),
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
        loop {
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
                if command.control.split(',').any(|field| field == "t=s") {
                    let frame = consume_shared_frame(&command)?;
                    self.frames.push(FrameObservation {
                        at: Instant::now(),
                        hash: hash_bytes(&frame.bytes),
                        size: frame.bytes.len(),
                    });
                }
            }
        }
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
                    "cellarium exited with {status} while waiting for {description}: {}",
                    String::from_utf8_lossy(&self.output)
                )));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for {description}; frames={}, output_bytes={}",
                        self.frames.len(),
                        self.output.len()
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
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        unsafe { libc::close(self.master) };
    }
}

pub fn run_terminal_probe(host: &str) -> io::Result<TerminalProbeReport> {
    let columns = 64;
    let rows = 20;
    let mut session = PtySession::spawn(host, columns, rows)?;
    session.pump_until("first consumed Kitty frame", STARTUP_TIMEOUT, |session| {
        !session.frames.is_empty()
    })?;

    let cadence_start = session.frames.len() - 1;
    session.pump_for(FRAME_WINDOW)?;
    let cadence_frames = &session.frames[cadence_start..];
    if cadence_frames.len() < 2 {
        return Err(io::Error::other("fewer than two Kitty frames consumed"));
    }
    let cadence_elapsed = cadence_frames
        .last()
        .unwrap()
        .at
        .duration_since(cadence_frames[0].at)
        .as_secs_f64();
    let kitty_frame_hz = (cadence_frames.len() - 1) as f64 / cadence_elapsed;

    let pause_started = Instant::now();
    session.write(b" ")?;
    session.pump_until("server pause acknowledgement", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"ack 1") && session.screen.contains(b"paused")
    })?;
    let pause_ack_at = Instant::now();
    let frames_after_pause_ack = session.frames.len();
    session.pump_until("post-pause Kitty frame", INPUT_TIMEOUT, |session| {
        consecutive_equal_frame(&session.frames, frames_after_pause_ack, None).is_some()
    })?;
    let pause_frame = consecutive_equal_frame(&session.frames, frames_after_pause_ack, None)
        .expect("stable post-pause frame");
    let paused_hash = pause_frame.hash;
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
    session.pump_until("stable cleared Kitty frame", INPUT_TIMEOUT, |session| {
        consecutive_equal_frame(&session.frames, frames_after_clear_ack, Some(paused_hash))
            .is_some()
    })?;
    let clear_frame =
        consecutive_equal_frame(&session.frames, frames_after_clear_ack, Some(paused_hash))
            .expect("stable cleared frame");
    let cleared_hash = clear_frame.hash;
    let clear_ack_latency_ms = clear_ack_at.duration_since(clear_started).as_secs_f64() * 1_000.0;
    let clear_frame_latency_ms =
        clear_frame.at.duration_since(clear_started).as_secs_f64() * 1_000.0;

    let mouse_started = Instant::now();
    // SGR mouse: button-motion bit plus left button, coordinates are 1-based.
    session.write(b"\x1b[<32;16;8M")?;
    session.pump_until("server mouse acknowledgement", INPUT_TIMEOUT, |session| {
        session.screen.contains(b"ack 3")
    })?;
    let mouse_ack_at = Instant::now();
    let frames_after_mouse_ack = session.frames.len();
    session.pump_until("mouse-edited Kitty frame", INPUT_TIMEOUT, |session| {
        session.frames[frames_after_mouse_ack..]
            .iter()
            .any(|frame| frame.hash != cleared_hash)
    })?;
    let mouse_frame = session.frames[frames_after_mouse_ack..]
        .iter()
        .find(|frame| frame.hash != cleared_hash)
        .copied()
        .expect("mouse-edited frame");
    let mouse_frame_latency_ms =
        mouse_frame.at.duration_since(mouse_started).as_secs_f64() * 1_000.0;
    let mouse_ack_latency_ms = mouse_ack_at.duration_since(mouse_started).as_secs_f64() * 1_000.0;

    let observed_frames = session.frames.len();
    let frame_intervals_ms = session
        .frames
        .windows(2)
        .map(|pair| pair[1].at.duration_since(pair[0].at).as_secs_f64() * 1_000.0)
        .collect();
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
    })
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
            "  \"frame_sizes\": [{}]\n",
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

fn consecutive_equal_frame(
    frames: &[FrameObservation],
    start: usize,
    different_from: Option<u64>,
) -> Option<FrameObservation> {
    frames.get(start..)?.windows(2).find_map(|pair| {
        (pair[0].hash == pair[1].hash
            && different_from.is_none_or(|previous| pair[1].hash != previous))
        .then_some(pair[1])
    })
}
