# Cellarium Cross-Platform Release Design

## Goal

Publish Cellarium from GitHub Actions as one executable per operating-system
and CPU-architecture target while preserving the current single-binary user
experience:

- direct terminal mode and C1 `server`/`connect` mode stay in the same program;
- Linux binaries contain both CPU and CUDA backends and select one at runtime;
- systems without a usable NVIDIA driver fall back to CPU automatically;
- non-CUDA platforms receive the same CLI and rendering features with the CPU
  backend;
- pushing a version tag creates a GitHub Release containing all supported
  binaries and checksums.

"One executable" is target-local: executable formats and machine code require
separate files for Linux, macOS, Windows, x86_64, and ARM64. Cellarium is not
split into separate viewer, server, CPU, or CUDA programs.

## Release targets

| Release asset | GitHub runner | Rust target | Backends in the executable |
| --- | --- | --- | --- |
| `cellarium-linux-x86_64` | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | CPU + CUDA |
| `cellarium-linux-aarch64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | CPU + CUDA |
| `cellarium-macos-x86_64` | `macos-15-intel` | `x86_64-apple-darwin` | CPU |
| `cellarium-macos-aarch64` | `macos-latest` | `aarch64-apple-darwin` | CPU |
| `cellarium-windows-x86_64` | `windows-latest` | `x86_64-pc-windows-msvc` | CPU |
| `cellarium-windows-aarch64` | `windows-11-arm` | `aarch64-pc-windows-msvc` | CPU |

Windows ARM64 is a GitHub-hosted public-preview runner. It remains a required
matrix member: a failure blocks publication rather than silently producing an
incomplete release.

The Linux ARM64 CUDA build is intended to support systems such as NVIDIA
Jetson. CI proves that the dynamically loaded CUDA code compiles on ARM64, but
cannot prove GPU execution because standard hosted runners have no NVIDIA GPU.

## Cargo and backend architecture

`cudarc` becomes an optional dependency behind a `cuda` Cargo feature. The
default feature set contains `cuda`, preserving the current behavior of plain
`cargo build` and local GPU installations.

Release jobs select features by target:

- Linux x86_64 and Linux ARM64 build with the default `cuda` feature.
- macOS and Windows build with `--no-default-features`.

The simulation backend keeps a stable public interface in both configurations.
With `cuda` enabled, `cuda_or_cpu` first constructs `CudaBackend` and falls back
to `CpuBackend` if dynamic CUDA driver/NVRTC loading or device initialization
fails. With `cuda` disabled, the same selection call deterministically returns
the CPU backend, while an explicit request for CUDA returns a clear
"CUDA support was not compiled in" error. Direct mode, `server`, and `connect`
therefore require no platform-specific branching.

CUDA remains dynamically loaded. Release binaries do not link to a CUDA SDK on
the GitHub runner and do not require CUDA merely to start.

## Continuous integration

`.github/workflows/ci.yml` runs for pull requests and pushes to `main` with
read-only repository permissions. It performs:

1. `cargo fmt --all -- --check`;
2. Linux CPU-only tests with `cargo test --all-targets --no-default-features`;
3. Linux default-feature tests/build to cover the CUDA-enabled fallback path;
4. CPU-only compile checks on the macOS and Windows target families.

The CUDA-enabled test job may execute on a runner without CUDA: Cellarium must
fall back to CPU and the test suite must still pass. Actual GPU execution is
verified on `tinker` before the changes are pushed.

## Release workflow

`.github/workflows/release.yml` runs for tags matching `v*` and has
`contents: write` permission. It first verifies that the tag is exactly
`v<package.version>` from `Cargo.toml`. A native-runner matrix then:

1. builds `cellarium` in release mode for its explicit Rust target;
2. stages exactly one executable for that target;
3. creates `.tar.gz` archives on Unix and `.zip` archives on Windows;
4. uploads the archive as a workflow artifact.

Only after every matrix member succeeds does a final job download all
artifacts, generate `SHA256SUMS`, and create the GitHub Release with the GitHub
CLI and automatically generated release notes. This avoids publishing a
partial release.

The initial publication uses the existing package version: push `main`, create
the annotated tag `v0.1.0`, and push that tag. No force push is permitted. If
the destination repository contains incompatible history, stop and report it
instead of overwriting it.

## Packaging and naming

Release archives use stable names independent of runner labels:

```text
cellarium-v0.1.0-linux-x86_64.tar.gz
cellarium-v0.1.0-linux-aarch64.tar.gz
cellarium-v0.1.0-macos-x86_64.tar.gz
cellarium-v0.1.0-macos-aarch64.tar.gz
cellarium-v0.1.0-windows-x86_64.zip
cellarium-v0.1.0-windows-aarch64.zip
SHA256SUMS
```

Each archive contains one executable named `cellarium` or `cellarium.exe`.
The executable itself continues to contain direct, server, and connect modes.

## Repository and commit handling

The existing uncommitted C1 remote-viewer work is preserved. The final history
uses focused commits:

1. the approved release design document;
2. the existing C1 remote-viewer implementation and documentation;
3. optional-CUDA portability, CI workflows, release workflow, and release
   documentation.

The remote named `origin` is set to
`https://github.com/wkj2333666/Cellarium.git`. After local and remote
verification, `main` and `v0.1.0` are pushed.

## Error handling and safety

- A build, test, packaging, version, or checksum failure prevents Release
  creation.
- CUDA absence is a runtime fallback condition, not a process-start failure.
- Explicit CUDA selection on a CPU-only build reports an actionable error.
- CI uses the workflow-provided token only for the final Release job.
- Workflow permissions are read-only except for the release job's required
  `contents: write` permission.
- Existing remote history is never force-updated.

## Verification and acceptance criteria

Before pushing:

- `cargo fmt --all -- --check` passes;
- `cargo test --all-targets --no-default-features` passes;
- default-feature tests pass on `tinker` with CUDA available;
- release builds succeed for every target that can be exercised locally;
- workflow YAML is parsed and checked for the expected triggers, permissions,
  matrix entries, feature flags, archive names, and release dependency graph;
- the installed `/home/wkj/.local/bin/cellarium` remains discoverable and its
  direct/server/connect CLI surface remains intact.

After pushing:

- GitHub CI succeeds on `main`;
- tag `v0.1.0` produces one archive for each of the six targets plus
  `SHA256SUMS`;
- Linux release binaries keep runtime CPU/CUDA fallback in one executable;
- no separate viewer/server or CPU/CUDA executable is introduced.
