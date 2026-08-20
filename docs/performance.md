# Performance notes

Cellarium records backend step and terminal render durations in `App::performance()`.
The TUI editor panel displays the most recent and cumulative average values, so
performance work starts from measurements taken on the same workload users see.

The main frame-time contributors are intentionally kept visible:

- backend stepping, including host/device transfers for CUDA;
- world rasterization and terminal drawing;
- input/event polling and the fixed 30 Hz simulation/render cadence.

The current implementation uses a bounded simulation backlog (at most eight
steps per iteration), synchronizes CUDA state before consuming it, and avoids
running a step when the backend reports an error. These are correctness and
latency safeguards; they are not substitutes for a GPU profiler.

For a local baseline, run:

```text
cargo test --release
cargo build --release
python3 /tmp/cellarium_smoke.py
```

Then run the release binary in a terminal wide enough to show the editor panel
and record the `step last/average` and `render last/average` values after the
simulation has settled. CUDA-specific kernel work should be evaluated with
Nsight Systems/Compute on the same world dimensions and rule file before any
optimization is accepted.
