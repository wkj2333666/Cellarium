# Cellarium C1 Remote Viewer Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox ( - [ ] ) syntax for tracking.

**Goal:** Add a local-rendered SSH viewer/server split without regressing direct local rendering, and install the server-capable binary under $HOME/.local/bin/cellarium on tinker.

**Architecture:** Keep App as the simulation authority. Add a framed remote protocol carrying snapshots and input. server runs an App loop without a terminal. connect launches the remote binary through SSH and drives a local terminal loop using the existing App/TUI and local ViewportDisplay. The local App mirrors commands for responsive UI and replaces its world cells from authoritative snapshots.

**Tech Stack:** Rust 2024, crossterm, ratatui, existing raster/display code, std::process SSH pipes, custom length-prefixed binary protocol, cargo test.

**Spec:** docs/superpowers/specs/2026-08-21-cellarium-c1-remote-viewer-design.md

## Global constraints

- Preserve all existing uncommitted direct-rendering fixes.
- Do not emit terminal control sequences from the server.
- Never block input handling on a graphics-frame write.
- Bound protocol lengths and reject malformed frames.
- Keep direct-mode CLI flags backward compatible.
- Use $HOME/.local/bin as the remote installation target.
- Verify every task at its stated boundary before proceeding.

## Tasks

### 1. Add protocol primitives and serialization tests

- [ ] Create src/remote.rs with protocol version, message tags, bounded frame read/write, command/mouse/input encoders, and snapshot encode/decode.
- [ ] Add unit tests for round-tripping commands, snapshots, malformed lengths, and EOF.
- [ ] Export the module from src/lib.rs.
- [ ] Run cargo test remote.

### 2. Add App snapshot/control helpers

- [ ] Expose a compact authoritative snapshot builder from App (world dimensions/cells, tick, paused state, rates, backend/rule/error text).
- [ ] Add safe helpers for replacing the mirrored world and applying remote status without changing direct-mode behavior.
- [ ] Add tests proving snapshot dimensions/cells and command forwarding semantics.

### 3. Add headless server loop

- [ ] Implement app::run_server using App plus the protocol reader/writer.
- [ ] Keep simulation stepping on a timed loop while input is read independently; use a reader thread/channel so a slow client cannot stop simulation.
- [ ] Send latest snapshots at a bounded cadence and flush after each complete frame.
- [ ] Exit cleanly on Quit/EOF and never touch crossterm terminal state.
- [ ] Add an in-memory server-loop test or protocol-level smoke test.

### 4. Add local viewer and SSH connector

- [ ] Implement app::run_connect(host, ssh_command) with a local Command child whose stdin/stdout are piped to <host> $HOME/.local/bin/cellarium server.
- [ ] Add a configurable CELLARIUM_SSH_COMMAND (default ssh) and clear errors when the executable or remote binary is missing.
- [ ] Run the existing TUI locally: enable raw mode/alternate screen/mouse capture, detect graphics locally, render snapshots at the local cadence, and send input immediately.
- [ ] Forward expression-editor keys, commands, mouse actions, and resize/camera metadata; apply snapshots to the mirrored App.
- [ ] Add tests for SSH command construction and viewer fallback behavior.

### 5. Extend CLI while preserving direct mode

- [ ] Add server and connect <host> subcommands plus --ssh-command parsing.
- [ ] Keep direct mode selected when no subcommand is present and retain all existing direct flags.
- [ ] Update usage/errors and add parser regression tests.
- [ ] Run cargo test --all-targets.

### 6. Add installation/documentation entrypoints

- [ ] Add scripts/install-local.sh that runs cargo install --path . --root "$HOME/.local" and verifies $HOME/.local/bin/cellarium.
- [ ] Document server setup and the kitten ssh override in README.md (or the existing user guide), including CELLARIUM_SSH_COMMAND='kitten ssh'.
- [ ] Keep the install script POSIX-safe and non-destructive.

### 7. Build, install, and verify on tinker

- [ ] Run cargo fmt --check, cargo test --all-targets, and cargo build --release.
- [ ] Run the install script from /home/wkj/projects/cellarium.
- [ ] Verify /home/wkj/.local/bin/cellarium server --help (or equivalent usage), command -v cellarium, and a bounded server protocol smoke test.
- [ ] Run git diff --check and report all test/build/install evidence.
