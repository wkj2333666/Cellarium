# Cellarium

Cellarium is a local application for designing and running lattice experiments:
Conway-style automata, Lenia and Orbium, periodic polygon tilings, editable
kernels and growth programs you write yourself.

It opens a native window, runs the simulation in the same process, and uses the
fastest compute backend the machine actually has. Nothing is sent anywhere.

```sh
cellarium                          # open the window
cellarium --experiment lenia.ron   # open the window on an experiment
cellarium --backend cpu            # run on the CPU, whatever the machine has
cellarium --safe-mode              # start without probing a GPU at all
cellarium --version
```

## Compute backends

Cellarium picks a backend at startup and falls back on its own when one is
unavailable:

1. **CUDA**, on an NVIDIA GPU with a working driver.
2. **wgpu**, on any GPU with Vulkan, Metal, DirectX 12 or OpenGL. This covers
   Apple Silicon, Intel and AMD integrated graphics, and Windows.
3. **CPU**, always available.

All three run the same compiled experiment, so a result does not depend on which
one was chosen. The backend in use is named in the status bar, and the Backend
panel lists every device found on the machine along with the reason any of them
was rejected. If a GPU probe is what hangs on a particular machine, start with
`--safe-mode` and change the setting from inside the window.

## Workspaces

- **Simulation** — run, step, reset, randomize and paint directly into the world.
- **Tiling** — design the periodic unit cell: draw polygons, use a preset, solve
  the seams that glue them together, and drag vertices with those seams held.
- **Channels** — add, colour, hide, freeze and delete the scalar fields, and
  preview either the running world or the values a run would start from.
- **Kernels** — edit the stencil of every kernel in a binding: paint weights,
  switch cells in and out of the support, and type exact values.
- **Growth** — write the program that turns kernel readings into an update, with
  a plot of what it actually does across the inputs it actually reads.
- **Experiment** — review the whole draft, see what stops it running, and go
  straight to the workspace that owns each problem.

Every editing operation has a control you can reach with the pointer. Keyboard
shortcuts only accelerate actions that are already visible.

## Building

Cellarium builds on stable Rust. The CUDA backend is on by default and is
optional:

```sh
cargo build --release                        # with CUDA
cargo build --release --no-default-features  # CPU and wgpu only
```

On Linux the window needs the usual X11/Wayland and GL development packages:

```sh
sudo apt-get install libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxkbcommon-dev libwayland-dev libgl1-mesa-dev mesa-vulkan-drivers
```

## Installing a release

Download the archive for your platform along with `SHA256SUMS`, then:

```sh
./scripts/install-gui-local.sh cellarium-v0.3.0-linux-x86_64.tar.gz SHA256SUMS
```

The installer verifies the checksum before it installs anything.

## Files

Experiments are RON files. Cellarium reads the older experiment format as well
as the current one, and opening a file never rewrites it on the way in. Saving
writes to a temporary file and renames it into place, so an interrupted save
leaves the previous file intact rather than a truncated one. Settings and a
recovery autosave live under `$XDG_DATA_HOME/cellarium`.

## Documentation

- [Releases](docs/releases.md)
- [Performance](docs/performance.md)
