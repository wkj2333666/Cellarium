use cellarium::input::Command;
use cellarium::remote::{InputMessage, RemoteMessage, Snapshot, read_message, write_message};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::io;
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const OBSERVATION_WINDOW: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub struct ProtocolProbeReport {
    pub host: String,
    pub protocol_version: u8,
    pub remote_binary_sha256: String,
    pub backend: String,
    pub observed_snapshots: usize,
    pub server_sim_hz: f64,
    pub snapshot_rx_hz: f64,
    pub server_reported_sim_hz: f64,
    pub server_last_step_ms: f64,
    pub server_average_step_ms: f64,
    pub server_step_samples: u64,
    pub pause_latency_ms: f64,
    pub step_latency_ms: f64,
    pub mouse_latency_ms: f64,
    pub snapshot_intervals_ms: Vec<f64>,
    pub tick_deltas: Vec<u64>,
}

struct TimedSnapshot {
    at: Instant,
    snapshot: Snapshot,
}

struct ProtocolConnection {
    child: Child,
    input: ChildStdin,
    updates: mpsc::Receiver<Result<TimedSnapshot, String>>,
    next_input_sequence: u64,
}

impl ProtocolConnection {
    fn connect(host: &str) -> io::Result<Self> {
        let direct = std::env::var_os("CELLARIUM_E2E_DIRECT_SERVER").is_some();
        let mut command = if direct {
            let client = std::env::var_os("CELLARIUM_E2E_CLIENT")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_cellarium").into());
            let mut command = ProcessCommand::new(client);
            command.arg("server");
            command
        } else {
            let mut command = ssh_command();
            command.args([host, "$HOME/.local/bin/cellarium", "server"]);
            command
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("SSH stdin was not piped"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("SSH stdout was not piped"))?;
        let (updates_tx, updates) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                match read_message(&mut output) {
                    Ok(Some(RemoteMessage::Snapshot(snapshot))) => {
                        if updates_tx
                            .send(Ok(TimedSnapshot {
                                at: Instant::now(),
                                snapshot,
                            }))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let _ = updates_tx.send(Err("remote protocol stream closed".into()));
                        break;
                    }
                    Err(error) => {
                        let _ = updates_tx.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });
        let mut connection = Self {
            child,
            input,
            updates,
            next_input_sequence: 1,
        };
        connection.send(RemoteMessage::Hello)?;
        connection.send(RemoteMessage::Viewport {
            width: 40,
            height: 14,
            // Deliberately use a non-half-block framebuffer. The server must
            // use these dimensions for the same screen->world mapping as the
            // Kitty client, otherwise a paint lands in a different cell.
            frame_width: 320,
            frame_height: 224,
        })?;
        Ok(connection)
    }

    fn send(&mut self, message: RemoteMessage) -> io::Result<()> {
        write_message(&mut self.input, &message).map_err(io::Error::other)
    }

    fn input(&mut self, input: InputMessage) -> io::Result<u64> {
        let sequence = self.next_input_sequence;
        self.send(RemoteMessage::Input { sequence, input })?;
        self.next_input_sequence = sequence.wrapping_add(1).max(1);
        Ok(sequence)
    }

    fn next_snapshot(&self, timeout: Duration) -> io::Result<TimedSnapshot> {
        match self.updates.recv_timeout(timeout) {
            Ok(Ok(snapshot)) => Ok(snapshot),
            Ok(Err(error)) => Err(io::Error::other(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "snapshot deadline"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "snapshot reader stopped",
            )),
        }
    }

    fn wait_for(
        &self,
        description: &str,
        timeout: Duration,
        predicate: impl Fn(&Snapshot) -> bool,
    ) -> io::Result<TimedSnapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timed out waiting for {description}"),
                ));
            }
            let snapshot = self.next_snapshot(remaining)?;
            if predicate(&snapshot.snapshot) {
                return Ok(snapshot);
            }
        }
    }
}

impl Drop for ProtocolConnection {
    fn drop(&mut self) {
        let _ = write_message(&mut self.input, &RemoteMessage::Quit);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn run_protocol_probe(host: &str) -> io::Result<ProtocolProbeReport> {
    let remote_binary_sha256 = remote_binary_sha256(host)?;
    let mut connection = ProtocolConnection::connect(host)?;
    let initial = connection.next_snapshot(SNAPSHOT_TIMEOUT)?;
    let backend = initial.snapshot.backend.clone();
    if !backend.contains("NVIDIA") && !backend.contains("CUDA") {
        return Err(io::Error::other(format!(
            "remote server reported non-GPU backend {backend:?}"
        )));
    }

    let observation_deadline = Instant::now() + OBSERVATION_WINDOW;
    let mut observations = vec![initial];
    while Instant::now() < observation_deadline {
        let remaining = observation_deadline.saturating_duration_since(Instant::now());
        match connection.next_snapshot(remaining) {
            Ok(snapshot) => observations.push(snapshot),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
            Err(error) => return Err(error),
        }
    }
    if observations.len() < 2 {
        return Err(io::Error::other("fewer than two snapshots observed"));
    }
    let snapshot_intervals_ms = observations
        .windows(2)
        .map(|pair| pair[1].at.duration_since(pair[0].at).as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    let tick_deltas = observations
        .windows(2)
        .map(|pair| pair[1].snapshot.tick.saturating_sub(pair[0].snapshot.tick))
        .collect::<Vec<_>>();
    let elapsed = observations
        .last()
        .unwrap()
        .at
        .duration_since(observations[0].at)
        .as_secs_f64();
    let tick_delta = observations
        .last()
        .unwrap()
        .snapshot
        .tick
        .saturating_sub(observations[0].snapshot.tick);
    let server_sim_hz = tick_delta as f64 / elapsed;
    let snapshot_rx_hz = (observations.len() - 1) as f64 / elapsed;
    let server_reported_sim_hz = observations.last().unwrap().snapshot.simulation_rate;
    let server_last_step_ms = observations.last().unwrap().snapshot.last_step_ms;
    let server_average_step_ms = observations.last().unwrap().snapshot.average_step_ms;
    let server_step_samples = observations.last().unwrap().snapshot.step_samples;

    let pause_started = Instant::now();
    let pause_sequence = connection.input(InputMessage::Command(Command::TogglePause))?;
    let paused = connection.wait_for("paused state ack", SNAPSHOT_TIMEOUT, |snapshot| {
        snapshot.paused && snapshot.applied_input_sequence >= pause_sequence
    })?;
    let pause_latency_ms = paused.at.duration_since(pause_started).as_secs_f64() * 1_000.0;

    let clear_sequence = connection.input(InputMessage::Command(Command::Clear))?;
    let cleared = connection.wait_for("cleared world", SNAPSHOT_TIMEOUT, |snapshot| {
        snapshot.paused
            && snapshot.applied_input_sequence >= clear_sequence
            && snapshot.cells.iter().all(|value| *value == 0.0)
    })?;

    let mouse_started = Instant::now();
    let mouse_sequence = connection.input(InputMessage::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 20,
        row: 7,
        modifiers: KeyModifiers::NONE,
    }))?;
    let painted = connection.wait_for("mouse paint", SNAPSHOT_TIMEOUT, |snapshot| {
        snapshot.paused
            && snapshot.applied_input_sequence >= mouse_sequence
            && snapshot
                .cells
                .get(72 * 256 + 50)
                .is_some_and(|value| *value >= 0.99)
            && snapshot
                .cells
                .iter()
                .enumerate()
                .all(|(index, value)| index == 72 * 256 + 50 || *value < 0.01)
    })?;
    let mouse_latency_ms = painted.at.duration_since(mouse_started).as_secs_f64() * 1_000.0;

    let second_clear_sequence = connection.input(InputMessage::Command(Command::Clear))?;
    let cleared_again =
        connection.wait_for("second cleared world", SNAPSHOT_TIMEOUT, |snapshot| {
            snapshot.paused
                && snapshot.applied_input_sequence >= second_clear_sequence
                && snapshot.tick <= cleared.snapshot.tick
                && snapshot.cells.iter().all(|value| *value == 0.0)
        })?;
    let tick_before_step = cleared_again.snapshot.tick;
    let step_started = Instant::now();
    let step_sequence = connection.input(InputMessage::Command(Command::Step))?;
    let stepped = connection.wait_for("single step", SNAPSHOT_TIMEOUT, |snapshot| {
        snapshot.paused
            && snapshot.applied_input_sequence >= step_sequence
            && snapshot.tick > tick_before_step
    })?;
    let step_latency_ms = stepped.at.duration_since(step_started).as_secs_f64() * 1_000.0;

    let unpause_sequence = connection.input(InputMessage::Command(Command::TogglePause))?;
    let _ = connection.wait_for("unpaused state", SNAPSHOT_TIMEOUT, |snapshot| {
        !snapshot.paused && snapshot.applied_input_sequence >= unpause_sequence
    })?;
    connection.send(RemoteMessage::Quit)?;

    Ok(ProtocolProbeReport {
        host: host.to_string(),
        protocol_version: cellarium::remote::PROTOCOL_VERSION,
        remote_binary_sha256,
        backend,
        observed_snapshots: observations.len(),
        server_sim_hz,
        snapshot_rx_hz,
        server_reported_sim_hz,
        server_last_step_ms,
        server_average_step_ms,
        server_step_samples,
        pause_latency_ms,
        step_latency_ms,
        mouse_latency_ms,
        snapshot_intervals_ms,
        tick_deltas,
    })
}

pub fn write_report(report: &ProtocolProbeReport) -> io::Result<()> {
    let path = std::env::var_os("CELLARIUM_E2E_REPORT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("target/e2e-tinker-protocol.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let intervals = report
        .snapshot_intervals_ms
        .iter()
        .map(|value| format!("{value:.6}"))
        .collect::<Vec<_>>()
        .join(", ");
    let ticks = report
        .tick_deltas
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let json = format!(
        concat!(
            "{{\n",
            "  \"host\": \"{}\",\n",
            "  \"protocol_version\": {},\n",
            "  \"remote_binary_sha256\": \"{}\",\n",
            "  \"backend\": \"{}\",\n",
            "  \"observed_snapshots\": {},\n",
            "  \"server_sim_hz\": {:.6},\n",
            "  \"snapshot_rx_hz\": {:.6},\n",
            "  \"server_reported_sim_hz\": {:.6},\n",
            "  \"server_last_step_ms\": {:.6},\n",
            "  \"server_average_step_ms\": {:.6},\n",
            "  \"server_step_samples\": {},\n",
            "  \"pause_latency_ms\": {:.6},\n",
            "  \"step_latency_ms\": {:.6},\n",
            "  \"mouse_latency_ms\": {:.6},\n",
            "  \"snapshot_intervals_ms\": [{}],\n",
            "  \"tick_deltas\": [{}]\n",
            "}}\n"
        ),
        json_string(&report.host),
        report.protocol_version,
        json_string(&report.remote_binary_sha256),
        json_string(&report.backend),
        report.observed_snapshots,
        report.server_sim_hz,
        report.snapshot_rx_hz,
        report.server_reported_sim_hz,
        report.server_last_step_ms,
        report.server_average_step_ms,
        report.server_step_samples,
        report.pause_latency_ms,
        report.step_latency_ms,
        report.mouse_latency_ms,
        intervals,
        ticks,
    );
    std::fs::write(path, json)
}

fn ssh_command() -> ProcessCommand {
    let ssh = std::env::var_os("CELLARIUM_E2E_SSH").unwrap_or_else(|| "ssh".into());
    let mut command = ProcessCommand::new(ssh);
    if let Some(config) = std::env::var_os("CELLARIUM_E2E_SSH_CONFIG") {
        command.arg("-F").arg(config);
    }
    command
}

fn remote_binary_sha256(host: &str) -> io::Result<String> {
    let output = if std::env::var_os("CELLARIUM_E2E_DIRECT_SERVER").is_some() {
        let client = std::env::var_os("CELLARIUM_E2E_CLIENT")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_cellarium").into());
        ProcessCommand::new("sha256sum").arg(client).output()?
    } else {
        ssh_command()
            .args([host, "sha256sum", "$HOME/.local/bin/cellarium"])
            .output()?
    };
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "remote sha256sum exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let checksum = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| io::Error::other("remote sha256sum returned no checksum"))?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other(
            "remote sha256sum returned an invalid checksum",
        ));
    }
    Ok(checksum.to_ascii_lowercase())
}

fn json_string(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            character => vec![character],
        })
        .collect()
}
