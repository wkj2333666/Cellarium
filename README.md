# Cellarium

> **Architecture migration notice (2026-08-27):** the next major version is
> replacing the terminal/SSH client-server interface with a local native
> egui/wgpu GUI and local CUDA → portable GPU → CPU compute fallback. The
> current v0.2.2 commands below describe the released legacy product. See
> [GUI migration handoff](docs/gui-migration-handoff.md) for the approved
> target and implementation entry points.

Cellarium is a GPU-accelerated cellular automata laboratory with an interactive
terminal UI. It supports Conway-style automata, Lenia/Orbium, editable kernels,
custom rule programs, and CPU/CUDA simulation backends.

Cellarium remains one executable with three launch modes:

```sh
cellarium                  # simulate and render in the current terminal
cellarium server           # headless C1 simulation server
cellarium connect tinker   # local renderer connected to a remote server
```

Graphics-capable terminals such as Kitty use high-precision graphics rendering
by default. For SSH sessions, `connect` renders locally so keyboard and mouse
input are not blocked by image frames crossing SSH. Native local Unix Kitty
viewers use shared-memory frame transfer instead of embedding full RGB frames
in the terminal byte stream, with inline Kitty graphics as a compatibility
fallback. See
[Remote viewer](docs/remote-viewer.md).

## Visual Workbench

Press `W` from the simulation to open the Workbench. Its left outline selects
World, Tiling, Channels, Kernels, Growth, or Experiment; the center is always
the primary graphics editor, and the right inspector reports exact metadata and
diagnostics. Click any region or use `Tab` to move focus, and press `?` for
gestures relevant to the visible editor.

- Tiling starts with one polygonal basis. Presets include square, triangles,
  regular hexagons, and octagon-square. Draw a custom polygon with `D`, click
  vertices, and finish with Enter/double-click. The strong center polygon is
  editable; translucent copies are its actual seam-derived neighbors.
- Channels start at one and must be added explicitly. One channel is rendered
  for maximum contrast on black; three channels default to RGB, and colors and
  visibility remain editable.
- Every `(basis polygon, output channel)` selects a complete RuleSet. It starts
  with one kernel. Clicking a translated polygon selects its semantic basis.
  Editing an inherited RuleSet detaches it by copy-on-write; reset-to-default
  relinks it.
- The kernel canvas draws real source polygons at their lattice offsets. Click
  or drag to select/paint a weight; wheel changes by `0.05`,
  `Shift+wheel` by `0.005`, and `Ctrl+wheel` by `0.5`. Press `E` or
  Enter for an exact floating-point value. Empty-canvas wheel zooms and middle
  drag pans.
- Growth shows the complete read-only signature
  `fn growth(self: Scalar, kernel_1, ...) -> Rate` above a multiline editor.
  Press `E` to edit. One kernel produces a precise curve; two kernels produce
  a 2D heatmap. The number of ordinary growth inputs always equals the selected
  RuleSet's kernel count.

Workbench changes are drafts. Review the Experiment section and press
`Ctrl+Enter` to Apply; server acknowledgement and the new authoritative
revision complete a remote Apply. `W` returns to simulation without confusing
draft edits with the active experiment.

## Build and install

On a Linux CUDA machine, the default build includes both CUDA and CPU backends
and selects one at runtime:

```sh
cargo build --release
./scripts/install-local.sh
```

To build without CUDA support:

```sh
cargo build --release --no-default-features
```

Prebuilt binaries for Linux, macOS, and Windows on x86_64 and ARM64 are
described in the [release installation guide](docs/releases.md).
