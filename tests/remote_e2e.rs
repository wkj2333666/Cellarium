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
fn public_protocol_roundtrips_normalized_basis_authority() {
    use cellarium::remote::{RemoteMessage, read_message, write_message};
    use cellarium::sim::experiment_model::ExperimentSpec;
    use cellarium::sim::tiling::{TilingPreset, build_preset};

    let mut experiment = ExperimentSpec::single_channel_lenia(2, 2);
    experiment.tiling = Some(build_preset(TilingPreset::EquilateralTriangles, 1.0));
    let experiment = experiment.normalize_rules().unwrap();
    let message = RemoteMessage::ExperimentState {
        revision: 21,
        normalized_experiment: experiment,
    };
    let mut bytes = Vec::new();
    write_message(&mut bytes, &message).unwrap();
    assert_eq!(
        read_message(&mut std::io::Cursor::new(bytes)).unwrap(),
        Some(message)
    );
}

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
fn terminal_screen_visual_hash_includes_ansi_colors() {
    let mut screen = terminal_probe::TerminalScreen::new(2, 1);
    screen.push(b"\x1b[1;1H\x1b[38;2;255;0;0mX");
    let red = screen.visual_hash(0, 0, 1, 1);
    screen.push(b"\x1b[1;1H\x1b[38;2;0;255;0mX");
    let green = screen.visual_hash(0, 0, 1, 1);
    assert_ne!(red, green);
}

#[test]
fn terminal_screen_visual_hash_distinguishes_half_block_glyphs() {
    let mut screen = terminal_probe::TerminalScreen::new(1, 1);
    screen.push("\u{2580}".as_bytes());
    let upper = screen.visual_hash(0, 0, 1, 1);
    screen.push(b"\x1b[1;1H");
    screen.push("\u{2584}".as_bytes());
    let lower = screen.visual_hash(0, 0, 1, 1);
    assert_ne!(upper, lower);
}

#[test]
fn terminal_probe_expands_kitty_rgb_frames_for_visual_checks() {
    assert_eq!(
        terminal_probe::decode_kitty_pixels(
            "a=T,f=24,t=s,s=2,v=1",
            2,
            1,
            &[255, 51, 0, 0, 255, 255],
        ),
        Some(vec![255, 51, 0, 255, 0, 255, 255, 255])
    );
}

#[cfg(feature = "cuda")]
#[test]
#[ignore = "requires tinker's NVIDIA driver and NVRTC"]
fn basis_cpu_cuda_parity() {
    use cellarium::sim::cpu::CpuExperimentBackend;
    use cellarium::sim::cuda::CudaExperimentBackend;
    use cellarium::sim::experiment_model::ExperimentSpec;
    use cellarium::sim::runtime::compile_experiment;
    use cellarium::sim::tiling::{TilingPreset, build_preset};
    use cellarium::sim::world::ChannelWorld;
    use cellarium::workbench::WorkbenchState;

    fn worlds(spec: &ExperimentSpec) -> (ChannelWorld, ChannelWorld) {
        let cell_count = spec.geometry.tile_count().unwrap();
        let bases = spec.basis_ids().len();
        let channels = spec
            .channels
            .iter()
            .map(|channel| {
                if channel.initial.len() == cell_count * bases {
                    channel.initial.clone()
                } else {
                    channel
                        .initial
                        .iter()
                        .flat_map(|value| std::iter::repeat_n(*value, bases))
                        .collect()
                }
            })
            .collect::<Vec<_>>();
        let width = match spec.geometry {
            cellarium::sim::experiment_model::GeometrySpec::RasterGrid(ref grid) => {
                grid.width as usize
            }
        };
        let height = cell_count / width;
        (
            ChannelWorld::from_basis_channels(width, height, bases, &channels).unwrap(),
            ChannelWorld::from_basis_channels(width, height, bases, &channels).unwrap(),
        )
    }

    fn assert_parity(spec: ExperimentSpec, label: &str) {
        let compiled = compile_experiment(&spec).unwrap();
        let mut cpu = CpuExperimentBackend::new(compiled.clone());
        let mut gpu = CudaExperimentBackend::new(compiled)
            .unwrap_or_else(|error| panic!("{label}: tinker CUDA unavailable: {error}"));
        let (mut cpu_world, mut gpu_world) = worlds(&spec);
        for _ in 0..3 {
            cpu.step(&mut cpu_world).unwrap();
            gpu.step(&mut gpu_world).unwrap();
        }
        for (index, (lhs, rhs)) in cpu_world.cells().iter().zip(gpu_world.cells()).enumerate() {
            assert!((lhs - rhs).abs() < 1e-5, "{label}[{index}]: {lhs} != {rhs}");
        }
    }

    for preset in [
        TilingPreset::Square,
        TilingPreset::EquilateralTriangles,
        TilingPreset::RegularHexagon,
        TilingPreset::OctagonSquare,
    ] {
        let mut spec = ExperimentSpec::single_channel_lenia(4, 3);
        spec.tiling = Some(build_preset(preset, 1.0));
        assert_parity(spec.normalize_rules().unwrap(), &format!("{preset:?}"));
    }

    let mut spec = ExperimentSpec::single_channel_lenia(4, 3);
    spec.tiling = Some(build_preset(TilingPreset::OctagonSquare, 1.0));
    let mut workbench = WorkbenchState::new(spec.normalize_rules().unwrap());
    workbench.add_channel().unwrap();
    workbench.add_channel().unwrap();
    workbench.add_kernel_for_selected().unwrap();
    assert_eq!(workbench.draft().channels.len(), 3);
    let selected = workbench.selected_rule_set().unwrap();
    assert_eq!(
        workbench.draft().rules.get(selected).unwrap().kernels.len(),
        2
    );
    assert_parity(workbench.draft().clone(), "three-channel two-kernel");
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
        report.kitty_frame_hz >= 20.0,
        "optimized Kitty terminal should sustain an interactive frame rate: {report:?}"
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
fn tinker_workbench_graphics_clears_when_switching_to_experiment() {
    let host = std::env::var("CELLARIUM_E2E_HOST").unwrap_or_else(|_| "tinker".into());
    terminal_probe::run_workbench_graphics_clear_probe(&host)
        .expect("Workbench graphics clear probe");
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

#[test]
#[ignore = "requires a configured SSH alias, PTY, and installed tinker server"]
fn tinker_workbench_fallback_remains_fully_interactive() {
    let host = std::env::var("CELLARIUM_E2E_HOST").unwrap_or_else(|_| "tinker".into());
    terminal_probe::run_workbench_fallback_probe(&host).expect("Workbench fallback PTY E2E probe");
}
