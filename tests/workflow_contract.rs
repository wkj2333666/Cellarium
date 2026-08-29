//! What the workflows and the packaged product must promise.
//!
//! These read the checked-in files as text. They cannot prove a workflow runs,
//! but they can prove it has not quietly lost a target, a gate or a platform,
//! and that nothing shipped still advertises the removed remote modes.

const CI: &str = include_str!("../.github/workflows/ci.yml");
const RELEASE: &str = include_str!("../.github/workflows/release.yml");
const SMOKE: &str = include_str!("../scripts/smoke-gui.sh");
const INSTALL: &str = include_str!("../scripts/install-gui-local.sh");
const DESKTOP: &str = include_str!("../packaging/cellarium.desktop");
const CARGO: &str = include_str!("../Cargo.toml");
const README: &str = include_str!("../README.md");
const RELEASES: &str = include_str!("../docs/releases.md");

/// Every target the product is released for.
const TARGETS: [&str; 6] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
];

#[test]
fn the_release_builds_every_supported_target() {
    for target in TARGETS {
        assert!(
            RELEASE.contains(target),
            "the release workflow no longer builds {target}"
        );
    }
}

#[test]
fn the_release_is_published_as_stable_with_checksums() {
    assert!(RELEASE.contains("sha256sum cellarium-* > SHA256SUMS"));
    assert!(RELEASE.contains("gh release create"));
    assert!(
        !RELEASE.contains("--prerelease"),
        "a release from a version tag is the stable release"
    );
    assert!(
        RELEASE.contains("--verify-tag"),
        "the tag must be the one that was tested"
    );
}

#[test]
fn the_package_version_is_a_stable_release_not_a_prerelease() {
    let version = CARGO
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .expect("Cargo.toml declares a version")
        .trim_matches('"');
    assert!(
        !version.contains('-'),
        "{version} carries a prerelease suffix"
    );
}

#[test]
fn ci_gates_the_cpu_only_configuration_as_well_as_the_default_one() {
    assert!(CI.contains("--no-default-features"));
    assert!(CI.contains("cargo fmt --all -- --check"));
    assert!(CI.contains("cargo clippy --locked --all-targets -- -D warnings"));
    assert!(RELEASE.contains("--no-default-features"));
}

#[test]
fn the_gui_is_built_on_a_machine_with_no_gpu() {
    // The CPU-only configuration is a real configuration of the product, not a
    // headless subset of it: the window is part of what it must build.
    let cpu_only = CI.split("--no-default-features").count().saturating_sub(1);
    assert!(
        cpu_only >= 2,
        "the CPU-only build must be tested and smoked"
    );
}

#[test]
fn linux_jobs_install_the_libraries_a_window_needs() {
    for library in ["libx11-dev", "libxkbcommon-dev", "libwayland-dev"] {
        assert!(CI.contains(library), "CI is missing {library}");
        assert!(
            RELEASE.contains(library),
            "the release is missing {library}"
        );
    }
    for runtime in ["mesa-vulkan-drivers", "libgl1-mesa-dev"] {
        assert!(CI.contains(runtime), "CI is missing {runtime}");
    }
    // Opened at run time rather than linked, so nothing about the build says it
    // is needed. This list said "the libraries a window needs" while missing the
    // one whose absence panics the window before it appears, and the smoke job
    // found it the first time it ever ran.
    assert!(
        CI.contains("libxkbcommon-x11-dev"),
        "the smoke job must install libxkbcommon-x11, which winit opens for the keyboard on X11"
    );
}

#[test]
fn the_documented_runtime_libraries_are_enough_to_open_a_window() {
    // A user who installs exactly what the release notes list has to end up with
    // a program that starts. libxkbcommon-x11 is a different package from
    // libxkbcommon and was absent here, so following the documentation to the
    // letter produced a panic rather than a window.
    for package in [
        "libx11-6",
        "libxkbcommon0",
        "libxkbcommon-x11-0",
        "libgl1",
        "mesa-vulkan-drivers",
    ] {
        assert!(
            RELEASES.contains(package),
            "the documented runtime libraries are missing {package}"
        );
    }
}

#[test]
fn ci_starts_the_window_and_proves_it_painted() {
    assert!(CI.contains("xvfb"), "the smoke job needs a display");
    assert!(CI.contains("scripts/smoke-gui.sh"));
    // A window that appears and paints nothing is not a working application.
    assert!(
        SMOKE.contains("distinct colours"),
        "the smoke test must check the window painted something"
    );
    assert!(
        SMOKE.contains("did not exit"),
        "the smoke test must require a clean exit rather than killing it"
    );
    assert!(
        SMOKE.contains("XDG_DATA_HOME"),
        "the smoke test must run against a clean data directory"
    );
}

#[test]
fn the_release_measures_backend_parity_without_letting_it_stall_publishing() {
    assert!(
        RELEASE.contains("backend_parity"),
        "a release must still measure parity on real hardware"
    );
    assert!(
        RELEASE.contains("runs-on: [self-hosted, cuda]"),
        "parity is only meaningful on a machine with a real device"
    );
    // Parity reports; it does not gate. `continue-on-error` already says a
    // parity failure must not sink a release, but it only covers a job that
    // runs and fails. A job whose labels match no online runner queues instead,
    // and GitHub waits a day before giving up — so listing it in `needs` left
    // the first release that included it sitting for twenty-four hours with
    // every artifact already built.
    assert!(
        RELEASE.contains("needs: [build]"),
        "publishing must not wait on a runner that may not exist"
    );
    assert!(
        !RELEASE.contains("needs: [build, parity]"),
        "publishing must not depend on parity"
    );
}

#[test]
fn a_linux_package_carries_what_a_desktop_needs_to_launch_it() {
    assert!(RELEASE.contains("cellarium.desktop"));
    assert!(DESKTOP.contains("Exec=cellarium"));
    assert!(DESKTOP.contains("Type=Application"));
    assert!(
        DESKTOP.contains("Terminal=false"),
        "this is not a terminal program any more"
    );
}

#[test]
fn the_installer_verifies_what_it_installs() {
    assert!(
        INSTALL.contains("sha256sum"),
        "an archive that arrived damaged must fail at install time"
    );
    assert!(
        INSTALL.contains("--version"),
        "the install is confirmed by running it"
    );
}

#[test]
fn nothing_that_ships_advertises_a_server_or_a_connect_mode() {
    for (name, text) in [
        ("ci.yml", CI),
        ("release.yml", RELEASE),
        ("smoke-gui.sh", SMOKE),
        ("install-gui-local.sh", INSTALL),
        ("cellarium.desktop", DESKTOP),
    ] {
        for banned in [
            "run_server",
            "run_connect",
            "--ssh-command",
            "cellarium server",
        ] {
            assert!(!text.contains(banned), "{name} still advertises `{banned}`");
        }
    }
}

#[test]
fn the_readme_describes_a_local_application() {
    let lowered = README.to_lowercase();
    assert!(
        lowered.contains("local"),
        "the README must say the product runs locally"
    );
    for banned in ["cellarium server", "cellarium connect", "--ssh-command"] {
        assert!(
            !lowered.contains(banned),
            "the README still documents `{banned}`"
        );
    }
}

#[test]
fn the_terminal_and_remote_dependencies_are_gone() {
    for banned in ["ratatui", "crossterm", "ratatui-image"] {
        assert!(
            !CARGO.contains(banned),
            "Cargo.toml still depends on {banned}"
        );
    }
    assert!(
        CARGO.contains("eframe"),
        "the GUI framework must be a dependency"
    );
}

/// The release notes name every asset by version, and a reader follows those
/// names literally — into `tar -xzf` and into the installer's argument list. A
/// documented version that is not the one being released sends them to a file
/// that does not exist. The runtime-library list in this same document had
/// already drifted out of truth once; this is the same failure waiting in the
/// same file.
#[test]
fn the_release_notes_name_the_version_actually_being_released() {
    let version = CARGO
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .expect("Cargo.toml declares a version")
        .trim_matches('"');
    let expected = format!("cellarium-v{version}-linux-x86_64.tar.gz");
    assert!(
        RELEASES.contains(&expected),
        "docs/releases.md does not mention {expected}; it still documents an older release"
    );
    let stale = RELEASES
        .lines()
        .find(|line| line.contains("cellarium-v") && !line.contains(&format!("v{version}")));
    assert!(
        stale.is_none(),
        "docs/releases.md still names an asset from another release: {}",
        stale.unwrap_or_default()
    );
}
