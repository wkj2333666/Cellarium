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
        .env("CELLARIUM_FAKE_INVOCATION", invocation)
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
