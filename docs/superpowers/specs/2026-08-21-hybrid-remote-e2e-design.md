# Hybrid Remote End-to-End Test Design

## Purpose

Cellarium's remote viewer currently exposes rates that can look healthy while
keyboard and mouse input remains delayed. The existing PTY smoke test proves
startup and basic input wiring, but it does not run through a real SSH server,
consume Kitty shared-memory frames, or distinguish target rates from observed
rates.

The new test must answer, from observable events:

1. Is tinker using its NVIDIA backend?
2. How quickly does the server advance the simulation?
3. How many snapshots reach the client?
4. How many Kitty frames are actually consumed?
5. How long do keyboard and mouse actions take to alter remote state and then
   become visible in a consumed frame?

## Performance boundary

tinker owns every simulation and throughput workload. The local ARM64 machine
only owns a small PTY, parses control bytes, injects input, and opens/unlinks
the shared-memory objects named by Kitty graphics commands. Tests use a small
terminal viewport so local image handling is negligible. No local simulation
rate, rendering benchmark, or CPU comparison is reported.

## Two complementary probes

### Protocol probe

The protocol probe starts the installed server through the same SSH alias used
by `cellarium connect`. It decodes snapshots and sends protocol input messages.
It records:

- backend name from the server snapshot;
- `server_sim_hz` from observed tick deltas and monotonic receive timestamps;
- `snapshot_rx_hz` from complete snapshot frames received;
- key and mouse `input_to_state_ms` from injection until the first snapshot
  that proves the requested state change.

This probe isolates server/input scheduling from terminal graphics behavior.

### Terminal probe

The terminal probe starts the actual `cellarium connect tinker` command in a
PTY with Kitty capability enabled. A minimal Kitty emulator parses graphics
commands. For every `t=s` transmission it:

1. decodes the POSIX shared-memory name;
2. opens and reads the exact advertised byte count;
3. unlinks the object, matching Kitty's shared-memory ownership contract;
4. timestamps the completed read as a consumed frame.

It injects ordinary key bytes and SGR mouse sequences into the PTY and records
`input_to_frame_ms` until a later frame reflecting the action is consumed.
`kitty_frame_hz` is computed only from completed shared-memory reads. This is a
consume rate, not a claim about compositor presentation.

## Metric semantics

Every displayed or reported rate has a source:

| Metric | Source | Meaning |
| --- | --- | --- |
| `server_sim_hz` | tinker snapshot ticks | completed simulation ticks per wall second |
| `server_step_ms` | tinker step timer | last/rolling average completed GPU step |
| `snapshot_rx_hz` | client protocol decoder | complete snapshots received per wall second |
| `ui_draw_hz` | client draw completion | completed terminal draws per wall second |
| `fresh_graphics_hz` | display encoder publish | draws containing a newly encoded viewport |
| `ui_draw_ms` | client draw timer | last/rolling average completed draw |
| `kitty_frame_hz` | E2E Kitty emulator | shared-memory frames fully read per wall second |

The C1 client performs viewport rasterization in a latest-only background
worker. The UI thread clones the current scalar snapshot (256×256 for the
default world) and remains available for input; if rasterization cannot keep
up, obsolete requests are replaced rather than queued.

Configured targets remain visible only when explicitly labelled `target`; they
must never be shown as observed performance.

## Reliability rules

- The test fails its precondition unless the installed tinker server reports an
  NVIDIA/CUDA backend.
- All waits use deadlines and include a bounded event trace on failure.
- Every input carries a monotonic sequence. Both probes require a snapshot with
  `applied_input_sequence` at or beyond that action before accepting its state
  or frame, so local optimistic changes and stale snapshots cannot pass.
- Warm-up samples are excluded and rates use monotonic timestamps.
- The JSON reports include host, binary checksum/protocol version where
  applicable, viewport, sample counts, latency, and all raw observation
  intervals needed to audit aggregates.
- The live-network test is ignored by normal `cargo test` and is invoked by a
  dedicated script with the SSH alias supplied explicitly.

## Initial acceptance gates

The first run is a diagnostic baseline, not a tuned performance promise. Hard
gates are correctness-oriented: GPU backend, successful keyboard and mouse
state transitions, continuously consumed frames, and internally consistent
observed metrics. Once the baseline identifies the dominant stage, regression
thresholds are set from repeated tinker runs with headroom rather than from the
local ARM64 host.
