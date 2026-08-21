const CI: &str = include_str!("../.github/workflows/ci.yml");
const PTY_TEST: &str = include_str!("pty_startup.rs");
const RELEASE: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn ci_checks_both_backend_configurations() {
    assert!(CI.contains("cargo test --locked --all-targets --no-default-features"));
    assert!(CI.contains("cargo test --locked --all-targets"));
    assert!(CI.contains("contents: read"));
}

#[test]
fn linux_specific_pty_suite_is_not_compiled_on_macos() {
    assert!(PTY_TEST.starts_with("#![cfg(target_os = \"linux\")]"));
}

#[test]
fn release_contains_every_supported_target() {
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
    ] {
        assert!(RELEASE.contains(target), "missing {target}");
    }
    assert!(RELEASE.contains("tags:\n      - 'v*'"));
    assert!(RELEASE.contains("contents: write"));
    assert!(RELEASE.contains("GH_REPO: ${{ github.repository }}"));
    assert!(RELEASE.contains("SHA256SUMS"));
    assert!(RELEASE.contains("needs: build"));
}

#[test]
fn linux_release_uses_an_old_glibc_baseline_for_server_compatibility() {
    assert!(
        RELEASE.matches("runner: ubuntu-22.04").count() >= 2,
        "both Linux archives must be linked against the Ubuntu 22.04 glibc baseline"
    );
    assert!(RELEASE.contains("gcc-aarch64-linux-gnu"));
    assert!(RELEASE.contains("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"));
}
