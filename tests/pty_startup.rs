#![cfg(target_os = "linux")]

use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use libc::{O_NOCTTY, O_RDWR, pollfd};

fn open_pty() -> (i32, i32) {
    let master = unsafe { libc::posix_openpt(O_RDWR | O_NOCTTY) };
    assert!(
        master >= 0,
        "openpt failed: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(unsafe { libc::grantpt(master) }, 0);
    assert_eq!(unsafe { libc::unlockpt(master) }, 0);

    let mut peer_name = [0 as libc::c_char; 128];
    assert_eq!(
        unsafe { libc::ptsname_r(master, peer_name.as_mut_ptr(), peer_name.len()) },
        0
    );
    let slave_path = unsafe {
        std::ffi::CStr::from_ptr(peer_name.as_ptr())
            .to_string_lossy()
            .into_owned()
    };
    let slave = unsafe { libc::open(peer_name.as_ptr(), O_RDWR | O_NOCTTY) };
    assert!(
        slave >= 0,
        "open {} failed: {}",
        slave_path,
        std::io::Error::last_os_error()
    );

    let winsize = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(unsafe { libc::ioctl(slave, libc::TIOCSWINSZ, &winsize) }, 0);
    (master, slave)
}

fn set_nonblocking(fd: i32) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
}

fn spawn_on_pty(slave: i32) -> std::process::Child {
    let stdin = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stdout = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stderr = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    Command::new(env!("CARGO_BIN_EXE_cellarium"))
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove("WEZTERM_EXECUTABLE")
        .env_remove("KONSOLE_VERSION")
        .env_remove("KITTY_WINDOW_ID")
        .spawn()
        .expect("spawn cellarium")
}

fn spawn_graphics_on_pty(slave: i32) -> std::process::Child {
    let stdin = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stdout = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stderr = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    Command::new(env!("CARGO_BIN_EXE_cellarium"))
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .env("TERM", "xterm-kitty")
        .env("SSH_CONNECTION", "127.0.0.1 22 127.0.0.1 50000")
        .env("CELLARIUM_REMOTE_GRAPHICS", "1")
        .env("CELLARIUM_CELL_WIDTH", "8")
        .env("CELLARIUM_CELL_HEIGHT", "16")
        .spawn()
        .expect("spawn cellarium graphics")
}

fn spawn_dynamic_graphics_on_pty(slave: i32) -> std::process::Child {
    use std::os::unix::process::CommandExt;

    let stdin = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stdout = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stderr = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let mut command = Command::new(env!("CARGO_BIN_EXE_cellarium"));
    command
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .env("TERM", "xterm-kitty")
        .env("SSH_CONNECTION", "127.0.0.1 22 127.0.0.1 50000")
        .env("CELLARIUM_REMOTE_GRAPHICS", "1")
        .env("CELLARIUM_E2E_TRACE", "1")
        .env_remove("TERM_PROGRAM")
        .env_remove("KITTY_WINDOW_ID")
        .env_remove("CELLARIUM_CELL_WIDTH")
        .env_remove("CELLARIUM_CELL_HEIGHT");
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(slave, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .expect("spawn dynamically sized graphics cellarium")
}

fn set_pty_size(fd: i32, columns: u16, rows: u16, pixel_width: u16, pixel_height: u16) {
    let winsize = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: pixel_width,
        ws_ypixel: pixel_height,
    };
    assert_eq!(unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &winsize) }, 0);
}

fn connector_fixture() -> (PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cellarium-connector-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let invocation = directory.join("invocation");
    for (name, status) in [("kitten", 23), ("ssh", 29)] {
        let path = directory.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{name}' \"$@\" > \"$CELLARIUM_FAKE_INVOCATION\"\nexit {status}\n"
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
    (directory, invocation)
}

fn server_connector_fixture() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cellarium-server-connector-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("ssh");
    std::fs::write(&path, "#!/bin/sh\nexec \"$CELLARIUM_TEST_BINARY\" server\n").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
    directory
}

fn spawn_connect_on_pty(
    slave: i32,
    fixture: &Path,
    invocation: &Path,
    explicit_command: Option<&Path>,
) -> std::process::Child {
    let stdin = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stdout = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let stderr = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
    let path = std::env::join_paths(std::iter::once(fixture.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cellarium"));
    command
        .args(["connect", "tinker"])
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .env("PATH", path)
        .env("TERM", "xterm-kitty")
        .env("KITTY_WINDOW_ID", "1")
        .env("CELLARIUM_CELL_WIDTH", "8")
        .env("CELLARIUM_CELL_HEIGHT", "16")
        .env("CELLARIUM_FAKE_INVOCATION", invocation)
        .env("CELLARIUM_TEST_BINARY", env!("CARGO_BIN_EXE_cellarium"))
        .env("CELLARIUM_E2E_TRACE", "1")
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .env_remove("CELLARIUM_SSH_COMMAND");
    if let Some(explicit_command) = explicit_command {
        command.env("CELLARIUM_SSH_COMMAND", explicit_command);
    }
    command.spawn().expect("spawn cellarium connector")
}

fn read_available(master: i32, output: &mut Vec<u8>) {
    let mut poll = pollfd {
        fd: master,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let ready = unsafe { libc::poll(&mut poll, 1, 0) };
        assert!(
            ready >= 0,
            "poll failed: {}",
            std::io::Error::last_os_error()
        );
        if ready == 0 || poll.revents & libc::POLLIN == 0 {
            return;
        }

        let mut buffer = [0_u8; 65536];
        let read = unsafe { libc::read(master, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            let error = std::io::Error::last_os_error();
            assert_eq!(
                error.raw_os_error(),
                Some(libc::EAGAIN),
                "read failed: {error}"
            );
            return;
        }
        if read == 0 {
            return;
        }
        output.extend_from_slice(&buffer[..read as usize]);
    }
}

fn contains(output: &[u8], needle: &[u8]) -> bool {
    output.windows(needle.len()).any(|window| window == needle)
}

fn kitty_transmit_sizes(output: &[u8]) -> Vec<(u32, u32)> {
    let text = String::from_utf8_lossy(output);
    text.split("\x1b_Gq=2,")
        .filter_map(|chunk| {
            let header = chunk.split(';').next()?;
            let width = header
                .split(',')
                .find_map(|part| part.strip_prefix("s="))?
                .parse()
                .ok()?;
            let height = header
                .split(',')
                .find_map(|part| part.strip_prefix("v="))?
                .parse()
                .ok()?;
            Some((width, height))
        })
        .collect()
}

fn pump_until(
    child: &mut std::process::Child,
    master: i32,
    output: &mut Vec<u8>,
    deadline: Instant,
    mut predicate: impl FnMut(&[u8]) -> bool,
) -> bool {
    while Instant::now() < deadline {
        read_available(master, output);
        if predicate(output) {
            return true;
        }
        if let Some(status) = child.try_wait().expect("check cellarium status") {
            read_available(master, output);
            let _ = status;
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn pump_until_exit(
    child: &mut std::process::Child,
    master: i32,
    output: &mut Vec<u8>,
    deadline: Instant,
) -> Option<std::process::ExitStatus> {
    while Instant::now() < deadline {
        read_available(master, output);
        if let Some(status) = child.try_wait().expect("check cellarium status") {
            read_available(master, output);
            return Some(status);
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn nonresponsive_terminal_startup_accepts_quit_and_restores_terminal() {
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_on_pty(slave);
    unsafe { libc::close(slave) };

    // Keep this terminal-startup regression independent of CPU simulation
    // speed on runners without CUDA.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);
    let mut output = Vec::new();
    let rendered = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| contains(output, b"Cellarium"),
    );
    assert!(
        rendered,
        "cellarium did not render on a nonresponsive terminal; output length: {}",
        output.len()
    );

    assert_eq!(unsafe { libc::write(master, b"q".as_ptr().cast(), 1) }, 1);
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
    );
    unsafe { libc::close(master) };

    let Some(status) = status else {
        child.kill().expect("kill cellarium");
        child.wait().expect("wait for killed cellarium");
        panic!(
            "cellarium ignored q after terminal capability query timeout; output length: {}",
            output.len()
        );
    };
    assert!(status.success(), "cellarium exited with {status}");
    assert!(contains(&output, b"\x1b[?1049l"));
    assert!(contains(&output, b"\x1b[?1000l"));
}

#[test]
fn kitty_connect_uses_plain_ssh_without_an_explicit_override() {
    let (fixture, invocation) = connector_fixture();
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_connect_on_pty(slave, &fixture, &invocation, None);
    unsafe { libc::close(slave) };

    let deadline = Instant::now() + Duration::from_secs(3);
    while !invocation.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let called = std::fs::read_to_string(&invocation).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    unsafe { libc::close(master) };
    let _ = std::fs::remove_dir_all(&fixture);

    assert_eq!(called, "ssh\ntinker\n$HOME/.local/bin/cellarium\nserver\n");
}

#[test]
fn connector_eof_reports_the_child_status_without_waiting_for_input() {
    let (fixture, invocation) = connector_fixture();
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child =
        spawn_connect_on_pty(slave, &fixture, &invocation, Some(&fixture.join("kitten")));
    unsafe { libc::close(slave) };

    let mut output = Vec::new();
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
    );
    if status.is_none() {
        child.kill().expect("kill hung connector viewer");
        child.wait().expect("wait for killed connector viewer");
    }
    unsafe { libc::close(master) };
    let _ = std::fs::remove_dir_all(&fixture);

    let status = status.expect("cellarium did not exit after the SSH connector closed stdout");
    assert!(!status.success());
    assert!(
        contains(&output, b"SSH connector exited with status 23"),
        "missing connector status in output: {}",
        String::from_utf8_lossy(&output)
    );
    assert!(!contains(&output, b"Broken pipe"));
}

#[test]
fn local_kitty_connect_keeps_control_responsive_with_shared_memory_frames() {
    let fixture = server_connector_fixture();
    let invocation = fixture.join("unused-invocation");
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_connect_on_pty(slave, &fixture, &invocation, None);
    unsafe { libc::close(slave) };

    let mut output = Vec::new();
    let shared_frame = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| contains(output, b"t=s"),
    );
    assert!(
        shared_frame,
        "C/S viewer did not emit a Kitty shared-memory frame"
    );
    assert!(
        !contains(&output, b"t=d"),
        "C/S viewer embedded Kitty pixels in the PTY"
    );

    let pressed = Instant::now();
    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);
    let paused = pump_until(
        &mut child,
        master,
        &mut output,
        pressed + Duration::from_secs(2),
        |output| contains(output, b"paused"),
    );
    assert!(
        paused,
        "C/S viewer did not process pause within two seconds"
    );

    assert_eq!(unsafe { libc::write(master, b"q".as_ptr().cast(), 1) }, 1);
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
    );
    unsafe { libc::close(master) };
    let _ = std::fs::remove_dir_all(&fixture);

    let Some(status) = status else {
        child.kill().expect("kill C/S viewer");
        child.wait().expect("wait for killed C/S viewer");
        panic!("C/S viewer ignored q after shared-memory rendering");
    };
    assert!(status.success(), "C/S viewer exited with {status}");
}

#[test]
fn c_s_pty_user_journey_survives_repeated_keyboard_and_mouse_operations() {
    let fixture = server_connector_fixture();
    let invocation = fixture.join("unused-invocation");
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_connect_on_pty(slave, &fixture, &invocation, None);
    unsafe { libc::close(slave) };
    thread::sleep(Duration::from_millis(100));
    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);

    let mut output = Vec::new();
    assert!(pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| contains(output, b"Cellarium") && contains(output, b"paused")
    ));

    // The editor entry must be discoverable and must not require a mouse
    // click on the side panel: this is the same key a user sees in the
    // footer/help text.
    assert_eq!(unsafe { libc::write(master, b"w".as_ptr().cast(), 1) }, 1);
    assert!(pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(2),
        |output| contains(output, b"Workbench") && contains(output, b"World")
    ));
    assert!(
        contains(&output, b"a=d,d=A,q=1"),
        "entering Workbench must delete the previous Kitty image placement"
    );

    // User-level Workbench journey: clicking the left outline must select a
    // section, clicking the canvas must edit the draft, and keyboard section
    // navigation must remain available after mouse interaction.
    let experiment_click = b"\x1b[<0;8;7M";
    assert_eq!(
        unsafe {
            libc::write(
                master,
                experiment_click.as_ptr().cast(),
                experiment_click.len(),
            )
        },
        experiment_click.len() as isize
    );
    assert!(
        pump_until(
            &mut child,
            master,
            &mut output,
            Instant::now() + Duration::from_secs(5),
            |output| contains(output, b"selected Experiment")
        ),
        "Workbench click did not select Experiment; tail={:?}",
        String::from_utf8_lossy(&output[output.len().saturating_sub(1200)..])
    );
    assert_eq!(unsafe { libc::write(master, b"t".as_ptr().cast(), 1) }, 1);
    assert!(pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(5),
        |output| contains(output, b"E2E_WORKBENCH_SECTION=World")
    ));
    let canvas_click = b"\x1b[<0;35;10M";
    assert_eq!(
        unsafe { libc::write(master, canvas_click.as_ptr().cast(), canvas_click.len()) },
        canvas_click.len() as isize
    );
    assert!(pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(5),
        |output| contains(output, b"E2E_WORKBENCH_DRAFT=Dirty")
    ));

    // Return to simulation, then exercise the high-frequency paths. SGR
    // mouse events use terminal coordinates (1-based); the viewport begins
    // inside a bordered panel, so this also covers the origin subtraction.
    assert_eq!(unsafe { libc::write(master, b"w".as_ptr().cast(), 1) }, 1);
    for key in b"1trackgv" {
        assert_eq!(unsafe { libc::write(master, [*key].as_ptr().cast(), 1) }, 1);
    }
    for _ in 0..8 {
        assert_eq!(unsafe { libc::write(master, b"n".as_ptr().cast(), 1) }, 1);
    }
    for index in 0..32_u16 {
        let column = 3 + (index % 24);
        let row = 3 + (index % 12);
        let event =
            format!("\x1b[<0;{column};{row}M\x1b[<32;{column};{row}M\x1b[<0;{column};{row}m");
        assert_eq!(
            unsafe { libc::write(master, event.as_ptr().cast(), event.len()) },
            event.len() as isize
        );
    }
    assert_eq!(unsafe { libc::write(master, b"q".as_ptr().cast(), 1) }, 1);
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(5),
    );
    unsafe { libc::close(master) };
    let _ = std::fs::remove_dir_all(&fixture);
    let Some(status) = status else {
        child.kill().expect("kill stalled C/S viewer");
        child.wait().expect("wait for stalled C/S viewer");
        panic!("C/S viewer froze after repeated keyboard/mouse operations");
    };
    assert!(status.success(), "C/S viewer exited with {status}");
}

#[test]
fn local_pty_user_journey_survives_a_burst_of_input() {
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_on_pty(slave);
    unsafe { libc::close(slave) };
    thread::sleep(Duration::from_millis(100));
    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);

    let mut output = Vec::new();
    assert!(pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| contains(output, b"Cellarium") && contains(output, b"paused")
    ));
    for _ in 0..40 {
        assert_eq!(unsafe { libc::write(master, b"t".as_ptr().cast(), 1) }, 1);
    }
    for index in 0..32_u16 {
        let column = 3 + (index % 24);
        let row = 3 + (index % 12);
        let event =
            format!("\x1b[<0;{column};{row}M\x1b[<32;{column};{row}M\x1b[<0;{column};{row}m");
        assert_eq!(
            unsafe { libc::write(master, event.as_ptr().cast(), event.len()) },
            event.len() as isize
        );
    }
    assert_eq!(unsafe { libc::write(master, b"q".as_ptr().cast(), 1) }, 1);
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
    );
    unsafe { libc::close(master) };
    let Some(status) = status else {
        child.kill().expect("kill stalled direct viewer");
        child.wait().expect("wait for stalled direct viewer");
        panic!("direct viewer froze after a burst of keyboard/mouse input");
    };
    assert!(status.success(), "direct viewer exited with {status}");
}

#[test]
fn remote_kitty_graphics_reencode_after_terminal_cell_size_changes() {
    let (master, slave) = open_pty();
    set_pty_size(slave, 80, 24, 800, 384);
    set_nonblocking(master);
    let mut child = spawn_dynamic_graphics_on_pty(slave);
    unsafe { libc::close(slave) };
    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);

    let mut output = Vec::new();
    let initial_frame = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| {
            kitty_transmit_sizes(output)
                .iter()
                .any(|(width, height)| width % 10 == 0 && height % 16 == 0)
        },
    );
    if !initial_frame {
        child
            .kill()
            .expect("kill viewer without initial Kitty frame");
        child
            .wait()
            .expect("wait for viewer without initial Kitty frame");
        unsafe { libc::close(master) };
        panic!(
            "no 10x16-cell Kitty frame; display_trace={:?}, sizes={:?}, bytes={}, tail={:?}",
            String::from_utf8_lossy(&output)
                .lines()
                .filter(|line| line.contains("E2E_"))
                .collect::<Vec<_>>(),
            kitty_transmit_sizes(&output),
            output.len(),
            String::from_utf8_lossy(&output[output.len().saturating_sub(800)..])
        );
    }

    read_available(master, &mut output);
    let resize_checkpoint = output.len();

    set_pty_size(master, 100, 30, 1100, 690);
    let resized = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(8),
        |output| {
            let resized_output = &output[resize_checkpoint.min(output.len())..];
            contains(resized_output, b"\x1b[2J")
                && kitty_transmit_sizes(resized_output)
                    .iter()
                    .any(|(width, height)| width % 11 == 0 && height % 23 == 0)
        },
    );

    child.kill().expect("stop dynamically resized viewer");
    child.wait().expect("wait for dynamically resized viewer");
    unsafe { libc::close(master) };

    assert!(
        resized,
        "no cleared 11x23-cell Kitty frame after PTY resize"
    );
}

#[test]
#[ignore = "direct Kitty graphics requires a draining terminal; use C1 connect over SSH"]
fn remote_graphics_startup_accepts_quit_without_waiting_for_frame_flush() {
    let (master, slave) = open_pty();
    set_nonblocking(master);
    let mut child = spawn_graphics_on_pty(slave);
    unsafe { libc::close(slave) };

    let mut output = Vec::new();
    let rendered = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(2),
        |output| !output.is_empty(),
    );
    assert!(
        rendered,
        "graphics mode produced no terminal output before timeout"
    );
    thread::sleep(Duration::from_millis(500));
    read_available(master, &mut output);
    assert_eq!(
        output
            .windows(b"\x1b[2J".len())
            .filter(|window| *window == b"\x1b[2J")
            .count(),
        0,
        "graphics mode emitted visible clear-screen sequences"
    );

    assert_eq!(unsafe { libc::write(master, b" ".as_ptr().cast(), 1) }, 1);
    let paused = pump_until(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
        |output| contains(output, b"paused"),
    );
    assert!(paused, "graphics mode did not process the pause key");

    assert_eq!(unsafe { libc::write(master, b"q".as_ptr().cast(), 1) }, 1);
    let status = pump_until_exit(
        &mut child,
        master,
        &mut output,
        Instant::now() + Duration::from_secs(3),
    );
    unsafe { libc::close(master) };

    let Some(status) = status else {
        child.kill().expect("kill graphics cellarium");
        child.wait().expect("wait for killed graphics cellarium");
        panic!("graphics mode ignored q while output was being flushed");
    };
    assert!(status.success(), "graphics cellarium exited with {status}");
}
