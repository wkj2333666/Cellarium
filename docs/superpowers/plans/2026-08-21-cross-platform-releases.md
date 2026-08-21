# Cellarium Cross-Platform Releases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish one Cellarium executable per supported OS/CPU target, with runtime CPU/CUDA selection inside each Linux executable, using GitHub Actions and a `v0.1.0` GitHub Release.

**Architecture:** Make CUDA an optional default Cargo feature and keep `SimulationBackend` as the target-independent boundary. Native GitHub-hosted runners build six release archives; a final job publishes them only after the full matrix succeeds.

**Tech Stack:** Rust 2024, Cargo features, cudarc dynamic loading, GitHub Actions, GitHub CLI, tar/zip, SHA-256.

**Spec:** `docs/superpowers/specs/2026-08-21-cross-platform-releases-design.md`

## Global Constraints

- Direct, `server`, and `connect` modes remain in one executable for every target.
- Linux x86_64 and ARM64 executables contain CPU and CUDA backends and fall back to CPU at runtime.
- macOS and Windows executables use the CPU backend and the same CLI surface.
- Release targets are Linux/macOS/Windows on x86_64 and ARM64.
- `v0.1.0` must match `package.version = "0.1.0"`.
- A failed matrix member prevents GitHub Release creation.
- Existing remote history is never force-updated.
- Existing C1 worktree changes must be preserved and committed separately from CI work.

---

### Task 1: Stabilize and commit the existing C1 remote viewer

**Files:**
- Modify if needed: `tests/pty_startup.rs`
- Commit existing: `src/app.rs`
- Commit existing: `src/lib.rs`
- Commit existing: `src/main.rs`
- Commit existing: `src/remote.rs`
- Commit existing: `src/render/display/mod.rs`
- Commit existing: `src/render/raster.rs`
- Commit existing: `tests/pty_startup.rs`
- Commit existing: `scripts/install-local.sh`
- Commit existing: `docs/remote-viewer.md`
- Commit existing: `docs/superpowers/specs/2026-08-21-cellarium-c1-remote-viewer-design.md`
- Commit existing: `docs/superpowers/plans/2026-08-21-cellarium-c1-remote-viewer.md`

**Interfaces:**
- Consumes: existing direct-mode `cellarium`, C1 protocol, and local installer.
- Produces: committed `cellarium server`, `cellarium connect <host>`, and `$HOME/.local/bin/cellarium` installation behavior on which release builds rely.

- [ ] **Step 1: Check the existing C1 patch for whitespace and formatting errors**

Run:

```bash
git diff --check
cargo fmt --all -- --check
```

Expected: both commands exit 0.

- [ ] **Step 2: Run the protocol and CLI tests**

Run:

```bash
cargo test --lib remote::
cargo test --bin cellarium
```

Expected: all C1 protocol and CLI parsing tests pass.

- [ ] **Step 3: Make the direct-over-SSH graphics stress test explicit if it is still flaky**

The C1 design supports SSH through `connect`; it does not promise that direct
Kitty frame writes to a non-draining remote PTY are interruptible. If the full
suite reproduces that known failure, annotate only that obsolete stress test:

```rust
#[test]
#[ignore = "direct Kitty graphics requires a draining terminal; use C1 connect over SSH"]
fn remote_graphics_startup_accepts_quit_without_waiting_for_frame_flush() {
```

Do not ignore protocol, CLI, half-block, or normal PTY tests.

- [ ] **Step 4: Run the complete current suite**

Run:

```bash
cargo test --all-targets
```

Expected: all non-ignored tests pass; at most the documented direct-Kitty stress test is ignored.

- [ ] **Step 5: Reinstall and smoke-test the server executable**

Run:

```bash
./scripts/install-local.sh
command -v cellarium
set +e
{ printf '\x43\x4c\x52\x4d\x01\x01\x00\x00\x00\x00'; sleep 3; } \
  | timeout 2 cellarium server >/tmp/cellarium-server.bin
statuses=("${PIPESTATUS[@]}")
set -e
test "${statuses[1]}" -eq 124
test -s /tmp/cellarium-server.bin
```

Expected: `command -v` prints `/home/wkj/.local/bin/cellarium`; after a
protocol Hello frame, the server runs until timeout and emits a non-empty
snapshot stream.

- [ ] **Step 6: Commit C1 independently**

```bash
git add src/app.rs src/lib.rs src/main.rs src/remote.rs src/render/display/mod.rs src/render/raster.rs tests/pty_startup.rs scripts/install-local.sh docs/remote-viewer.md docs/superpowers/specs/2026-08-21-cellarium-c1-remote-viewer-design.md docs/superpowers/plans/2026-08-21-cellarium-c1-remote-viewer.md
git commit -m "feat: add low-latency remote viewer"
```

Expected: the commit contains C1 implementation and documentation but no CI files.

### Task 2: Make CUDA optional without splitting the executable

**Files:**
- Modify: `Cargo.toml`
- Create: `src/sim/backend_error.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/sim/backend.rs`
- Modify: `src/sim/cuda.rs`
- Modify: `src/app.rs`

**Interfaces:**
- Consumes: `SimulationSpec`, `CpuBackend`, and the existing `CudaBackend` implementation.
- Produces: Cargo feature `cuda` (enabled by default), `BackendError::CudaNotCompiled`, and unchanged `SimulationBackend::{cpu,cuda_or_cpu,strict_for_kind}` call signatures.

- [ ] **Step 1: Add a failing CPU-only backend contract test**

Add to `src/sim/backend.rs` tests:

```rust
#[cfg(not(feature = "cuda"))]
#[test]
fn cpu_only_build_falls_back_and_rejects_explicit_cuda() {
    let selected = SimulationBackend::cuda_or_cpu(SimulationSpec::conway(), 8, 8);
    assert_eq!(selected.kind(), BackendKind::Cpu);

    let strict = SimulationBackend::strict_for_kind(
        BackendKind::Cuda,
        SimulationSpec::conway(),
        8,
        8,
    );
    assert!(matches!(strict, Err(BackendError::CudaNotCompiled)));
}
```

- [ ] **Step 2: Run the new configuration to verify RED**

Run:

```bash
cargo test --no-default-features cpu_only_build_falls_back_and_rejects_explicit_cuda
```

Expected: compilation fails because the `cuda` feature and `CudaNotCompiled` variant do not exist yet.

- [ ] **Step 3: Declare the optional default CUDA feature**

Change `Cargo.toml` to include:

```toml
[features]
default = ["cuda"]
cuda = ["dep:cudarc"]

[dependencies]
cudarc = { version = "0.19.9", optional = true, default-features = false, features = ["std", "driver", "nvrtc", "fallback-dynamic-loading", "cuda-11080"] }
```

Keep all other dependency versions unchanged.

- [ ] **Step 4: Extract the shared backend error**

Create `src/sim/backend_error.rs` with one `BackendError` enum. Preserve the
existing rule/codegen/cache/world/topology variants, gate cudarc-backed
driver/compile variants with `#[cfg(feature = "cuda")]`, and add:

```rust
#[error("CUDA support was not compiled in")]
CudaNotCompiled,
```

In `src/sim/cuda.rs`, remove the old enum and import:

```rust
pub use super::backend_error::BackendError;
```

This keeps CUDA builds source-compatible while allowing CPU-only builds to
name the common error without compiling cudarc.

- [ ] **Step 5: Gate CUDA modules and variants**

In `src/sim/mod.rs` declare `pub mod backend_error;` and gate `pub mod cuda;`
with `#[cfg(feature = "cuda")]`.

In `src/sim/backend.rs`:

```rust
pub use super::backend_error::BackendError;
#[cfg(feature = "cuda")]
use super::cuda::CudaBackend;

pub enum SimulationBackend {
    Cpu(Box<CpuBackend>),
    #[cfg(feature = "cuda")]
    Cuda(Box<CudaBackend>),
}
```

Use `#[cfg(feature = "cuda")]` arms for CUDA construction and delegation. In
the disabled configuration, `cuda_or_cpu` returns `Self::cpu(spec)` and
`strict_for_kind(BackendKind::Cuda, ..)` returns
`Err(BackendError::CudaNotCompiled)`.

- [ ] **Step 6: Gate the direct CudaBackend test probe in app tests**

Define both configurations in `src/app.rs` tests:

```rust
#[cfg(feature = "cuda")]
fn cuda_available() -> bool {
    crate::sim::cuda::CudaBackend::new(SimulationSpec::conway(), 1, 1).is_ok()
}

#[cfg(not(feature = "cuda"))]
fn cuda_available() -> bool {
    false
}
```

No non-test application path may import `crate::sim::cuda` directly.

- [ ] **Step 7: Run CPU-only and CUDA-enabled suites**

Run:

```bash
cargo test --all-targets --no-default-features
cargo test --all-targets
```

Expected: both configurations pass; default-feature tests use CUDA on `tinker` when available, and CPU-only tests never compile cudarc.

- [ ] **Step 8: Commit backend portability**

```bash
git add Cargo.toml Cargo.lock src/sim/backend_error.rs src/sim/mod.rs src/sim/backend.rs src/sim/cuda.rs src/app.rs
git commit -m "feat: make CUDA an optional backend"
```

### Task 3: Add testable CI and release workflows

**Files:**
- Create: `tests/workflow_contract.rs`
- Modify: `tests/pty_startup.rs`
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Cargo feature `cuda`, target-independent CLI, package version, GitHub tag name, and workflow token.
- Produces: main/PR CI, six target archives, `SHA256SUMS`, and an atomic GitHub Release publication dependency graph.

- [ ] **Step 1: Add failing workflow contract tests**

Create `tests/workflow_contract.rs`:

```rust
const CI: &str = include_str!("../.github/workflows/ci.yml");
const RELEASE: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn ci_checks_both_backend_configurations() {
    assert!(CI.contains("cargo test --locked --all-targets --no-default-features"));
    assert!(CI.contains("cargo test --locked --all-targets"));
    assert!(CI.contains("contents: read"));
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
    assert!(RELEASE.contains("SHA256SUMS"));
    assert!(RELEASE.contains("needs: build"));
}
```

- [ ] **Step 2: Verify the contract test is RED**

Run:

```bash
cargo test --test workflow_contract
```

Expected: compilation fails because both workflow files are absent.

- [ ] **Step 3: Make the Unix-only PTY test target explicit**

Add this as the first line of `tests/pty_startup.rs`:

```rust
#![cfg(unix)]
```

This prevents Windows `--all-targets` checks from compiling POSIX PTY calls.

- [ ] **Step 4: Create the CI workflow**

Create `.github/workflows/ci.yml` with:

- triggers for pull requests, pushes to `main`, and manual dispatch;
- top-level `permissions: contents: read`;
- an Ubuntu formatting plus CPU/default-feature test job using the exact two
  commands asserted above;
- a portability matrix for `macos-15-intel`, `macos-latest`,
  `windows-latest`, and `windows-11-arm` running
  `cargo check --locked --all-targets --no-default-features --target <target>`;
- `actions/checkout@v4` and the runners' installed stable Rust toolchain.

The CUDA-enabled test command must run on Ubuntu without installing CUDA so it
exercises runtime CPU fallback.

- [ ] **Step 5: Create the release workflow**

Create `.github/workflows/release.yml` with three jobs:

1. `version` checks `GITHUB_REF_NAME` equals `v` plus the first package version
   in `Cargo.toml`.
2. `build` needs `version`, uses the six native runner/target entries from the
   spec, builds Linux with default features and macOS/Windows with
   `--no-default-features`, packages one executable, and uploads one archive
   through `actions/upload-artifact@v4`.
3. `release` needs `build`, downloads all archives with
   `actions/download-artifact@v4`, runs
   `sha256sum cellarium-* > SHA256SUMS`, and executes:

```bash
gh release create "$GITHUB_REF_NAME" cellarium-* SHA256SUMS --verify-tag --generate-notes
```

Set only the final job to `permissions: contents: write`. Use `.tar.gz` on
Unix and `.zip` on Windows with the exact names in the spec.

- [ ] **Step 6: Run workflow contract and all local tests**

Run:

```bash
cargo test --test workflow_contract
cargo test --locked --all-targets --no-default-features
cargo test --locked --all-targets
```

Expected: the workflow contract and both backend configurations pass.

- [ ] **Step 7: Commit workflows**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml tests/workflow_contract.rs tests/pty_startup.rs
git commit -m "ci: publish cross-platform release binaries"
```

### Task 4: Document release installation and verify packaging

**Files:**
- Create: `README.md`
- Create: `docs/releases.md`

**Interfaces:**
- Consumes: stable archive names and existing `cellarium` CLI modes.
- Produces: discoverable install instructions and backend behavior for Release users.

- [ ] **Step 1: Add concise repository and release documentation**

`README.md` must identify Cellarium, list direct/server/connect modes, link to
`docs/remote-viewer.md` and `docs/releases.md`, and state that graphics-capable
terminals default to high-precision rendering.

`docs/releases.md` must list all six archive names, explain that each archive
contains one executable, document Linux runtime CUDA-to-CPU fallback, and show:

```bash
tar -xzf cellarium-v0.1.0-linux-x86_64.tar.gz
install -Dm755 cellarium "$HOME/.local/bin/cellarium"
```

For Windows, document extracting `cellarium.exe` from the zip. Do not claim
CUDA support for macOS or Windows.

- [ ] **Step 2: Build and package the local Linux x86_64 artifact exactly once**

Run:

```bash
cargo build --locked --release --target x86_64-unknown-linux-gnu
mkdir -p target/release-smoke
cp target/x86_64-unknown-linux-gnu/release/cellarium target/release-smoke/cellarium
tar -C target/release-smoke -czf target/cellarium-v0.1.0-linux-x86_64.tar.gz cellarium
tar -tzf target/cellarium-v0.1.0-linux-x86_64.tar.gz
sha256sum target/cellarium-v0.1.0-linux-x86_64.tar.gz
```

Expected: archive listing contains exactly `cellarium`, and SHA-256 generation succeeds.

- [ ] **Step 3: Final local verification**

Run:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --locked --all-targets --no-default-features
cargo test --locked --all-targets
./scripts/install-local.sh
command -v cellarium
git status --short
```

Expected: checks pass, installation resolves to `/home/wkj/.local/bin/cellarium`, and only intended documentation changes remain uncommitted.

- [ ] **Step 4: Commit release documentation**

```bash
git add README.md docs/releases.md
git commit -m "docs: add release installation guide"
```

### Task 5: Push and verify the published release

**Files:**
- Modify Git metadata: `.git/config`, `refs/tags/v0.1.0`
- External result: `https://github.com/wkj2333666/Cellarium`

**Interfaces:**
- Consumes: clean verified `main`, GitHub credentials, and release workflow.
- Produces: remote `main`, tag `v0.1.0`, passing Actions runs, and seven Release assets.

- [ ] **Step 1: Inspect destination history before configuring origin**

Run:

```bash
git ls-remote https://github.com/wkj2333666/Cellarium.git
git remote -v
```

Expected: the repository is empty or its `main` is compatible with local
history. Stop without force pushing if it has incompatible commits.

- [ ] **Step 2: Configure and push main**

Run one applicable remote command:

```bash
git remote add origin https://github.com/wkj2333666/Cellarium.git
```

or, if `origin` already exists:

```bash
git remote set-url origin https://github.com/wkj2333666/Cellarium.git
```

Then run:

```bash
git push -u origin main
```

Expected: `origin/main` points to the verified local `main` commit.

- [ ] **Step 3: Observe main CI before tagging**

Run:

```bash
gh run list --repo wkj2333666/Cellarium --branch main --limit 5
gh run watch --repo wkj2333666/Cellarium --exit-status
```

Expected: the main CI run completes successfully. If it fails, inspect the
failed job, fix it in a new commit, re-run local verification, and push that
commit before tagging.

- [ ] **Step 4: Create and push the initial release tag**

Run:

```bash
git tag -a v0.1.0 -m "Cellarium v0.1.0"
git push origin v0.1.0
```

Expected: the release workflow starts for the exact verified main commit.

- [ ] **Step 5: Observe release publication and verify assets**

Run:

```bash
gh run watch --repo wkj2333666/Cellarium --exit-status
gh release view v0.1.0 --repo wkj2333666/Cellarium
```

Expected: the Release contains six platform archives and `SHA256SUMS`; no
separate CPU, CUDA, viewer, or server executable is present.
