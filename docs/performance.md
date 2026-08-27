# Performance notes

Cellarium runs the simulation on a worker thread and draws the window on the
main thread. The two are deliberately independent: the window keeps responding
to the pointer while a slow backend takes as long as it takes, and a fast
backend is never held to the frame rate.

The status bar reports both rates separately, which is what makes the split
visible:

- **sim Hz** — how often the worker completed a step.
- **frame Hz** — how often the window was drawn.

A slow rate is reported as a decimal rather than rounded to zero. "0 Hz" beside
a visibly advancing simulation is a bug report waiting to happen.

## What costs time

- Backend stepping, including host/device transfers for CUDA and wgpu.
- Texture upload of the world, which happens once per published snapshot rather
  than once per frame. A paused simulation uploads nothing.
- Canvas painting. The tiling canvas triangulates every polygon it fills, so a
  cell with many vertices costs more than a square one.

The worker publishes snapshots into a bounded slot rather than a queue: when
the window cannot keep up, frames are dropped instead of accumulating latency,
so what is on screen is the newest state the worker has produced.

## Measuring

```sh
cargo build --release
./target/release/cellarium --experiment docs/examples/lenia.ron
```

Run the simulation until the rates settle, then read `sim Hz` and `frame Hz`
from the status bar. Compare backends by choosing each explicitly rather than
relying on the automatic choice:

```sh
cellarium --backend cuda
cellarium --backend wgpu
cellarium --backend cpu
```

All three run the same compiled experiment, so a difference in output between
them is a defect rather than a tuning result. `cargo test --test backend_parity`
checks that on whatever devices the machine has.

CUDA kernel work should be evaluated with Nsight Systems or Nsight Compute on
the same world dimensions and rule file before any optimization is accepted.
