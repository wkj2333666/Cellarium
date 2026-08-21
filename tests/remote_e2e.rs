#![cfg(target_os = "linux")]

#[path = "support/kitty_terminal.rs"]
mod kitty_terminal;
#[path = "support/remote_probe.rs"]
mod remote_probe;
#[path = "support/terminal_probe.rs"]
mod terminal_probe;

use base64::Engine;
use kitty_terminal::{KittyCommand, KittyStreamParser, consume_shared_frame};
use std::ffi::CString;

#[test]
fn kitty_parser_handles_split_and_coalesced_apc_commands() {
    let mut parser = KittyStreamParser::default();
    assert!(parser.push(b"prefix\x1b_Ga=T,t=s,S=4;").is_empty());

    let commands = parser.push(b"L3RtcC9h\x1b\\middle\x1b_Ga=d,d=I,i=7\x1b\\");

    assert_eq!(
        commands,
        vec![
            KittyCommand {
                control: "a=T,t=s,S=4".into(),
                payload: "L3RtcC9h".into(),
            },
            KittyCommand {
                control: "a=d,d=I,i=7".into(),
                payload: String::new(),
            },
        ]
    );
}

#[test]
fn kitty_parser_ignores_non_graphics_escape_sequences() {
    let mut parser = KittyStreamParser::default();
    let commands = parser.push(b"\x1b[2JCellarium\x1b[?25l");
    assert!(commands.is_empty());
}

#[test]
fn kitty_consumer_reads_exact_bytes_and_unlinks_shared_memory() {
    let name = format!(
        "/cellarium-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let c_name = CString::new(name.clone()).unwrap();
    let fd = unsafe {
        libc::shm_open(
            c_name.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
            0o600,
        )
    };
    assert!(fd >= 0, "shm_open: {}", std::io::Error::last_os_error());
    let bytes = [1_u8, 2, 3, 4];
    assert_eq!(
        unsafe { libc::ftruncate(fd, bytes.len() as libc::off_t) },
        0
    );
    assert_eq!(
        unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) },
        bytes.len() as isize
    );
    unsafe { libc::close(fd) };

    let command = KittyCommand {
        control: "a=T,t=s,S=4,f=32,s=1,v=1".into(),
        payload: base64::engine::general_purpose::STANDARD_NO_PAD.encode(name.as_bytes()),
    };
    let consumed = consume_shared_frame(&command).expect("consume frame");

    assert_eq!(consumed.name, name);
    assert_eq!(consumed.bytes, bytes);
    let reopened = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    assert_eq!(reopened, -1, "consumer must unlink Kitty-owned shm");
}

#[test]
fn terminal_screen_reconstructs_differential_ack_updates() {
    let mut screen = terminal_probe::TerminalScreen::new(16, 2);
    screen.push(b"\x1b[2;1Hack 0 - running");
    screen.push(b"\x1b[2;5H1\x1b_Ga=T,t=s,S=4;L3RtcC94\x1b\\");

    assert!(screen.contains(b"ack 1"));
    assert!(!screen.contains(b"ack 0"));
}

#[test]
#[ignore = "requires a configured SSH alias and the installed tinker server"]
fn tinker_protocol_observes_gpu_rates_and_input_latency() {
    let host = std::env::var("CELLARIUM_E2E_HOST").unwrap_or_else(|_| "tinker".into());
    let report = remote_probe::run_protocol_probe(&host).expect("protocol E2E probe");
    assert!(
        report.backend.contains("NVIDIA") || report.backend.contains("CUDA"),
        "remote server must use its GPU, got {}",
        report.backend
    );
    assert!(
        report.observed_snapshots >= 20,
        "too few snapshots: {report:?}"
    );
    assert!(
        report.server_sim_hz > 0.0,
        "simulation did not advance: {report:?}"
    );
    assert!(
        report.server_step_samples > 0,
        "server step timer was not populated: {report:?}"
    );
    assert_eq!(report.protocol_version, cellarium::remote::PROTOCOL_VERSION);
    assert_eq!(report.remote_binary_sha256.len(), 64);
    assert!(
        report.pause_latency_ms.is_finite(),
        "pause was not observed"
    );
    assert!(report.step_latency_ms.is_finite(), "step was not observed");
    assert!(
        report.mouse_latency_ms.is_finite(),
        "mouse edit was not observed"
    );
    remote_probe::write_report(&report).expect("write E2E JSON report");
    println!("{report:#?}");
}

#[test]
#[ignore = "requires a configured SSH alias, PTY, and installed tinker server"]
fn tinker_terminal_consumes_kitty_frames_and_observes_input() {
    let host = std::env::var("CELLARIUM_E2E_HOST").unwrap_or_else(|_| "tinker".into());
    let report = terminal_probe::run_terminal_probe(&host).expect("terminal E2E probe");
    assert!(
        report.observed_frames >= 10,
        "too few consumed frames: {report:?}"
    );
    assert!(
        report.kitty_frame_hz > 0.0,
        "no Kitty frame cadence: {report:?}"
    );
    assert!(
        report.pause_ack_latency_ms.is_finite(),
        "pause server ack was not observed"
    );
    assert!(
        report.pause_text_latency_ms.is_finite(),
        "pause text was not observed"
    );
    assert!(
        report.pause_frame_latency_ms.is_finite(),
        "pause frame was not observed"
    );
    assert!(
        report.clear_ack_latency_ms.is_finite(),
        "clear server ack was not observed"
    );
    assert!(
        report.mouse_ack_latency_ms.is_finite(),
        "mouse server ack was not observed"
    );
    assert!(
        report.mouse_frame_latency_ms.is_finite(),
        "mouse frame was not observed"
    );
    terminal_probe::write_report(&report).expect("write terminal E2E JSON report");
    println!("{report:#?}");
}

#[test]
#[ignore = "requires a configured SSH alias, PTY, and installed tinker server"]
fn tinker_workbench_user_journey_applies_authoritatively() {
    let host = std::env::var("CELLARIUM_E2E_HOST").unwrap_or_else(|_| "tinker".into());
    let latency = terminal_probe::run_workbench_probe(&host).expect("Workbench PTY E2E probe");
    assert!(
        latency.is_finite() && latency < 20_000.0,
        "Apply latency: {latency}ms"
    );
    println!("Workbench ApplyAccepted latency: {latency:.1}ms");
}
