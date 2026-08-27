# Cellarium GUI Migration Handoff

> Date: 2026-08-27
> Current product version: v0.2.2
> Current working branch: `basis-workbench-implementation`
> GUI migration status: design and implementation planning are complete; migration code has not started

## 1. Settled product decisions

These decisions are final unless the user explicitly changes them.

1. Migrate the primary interface to a native GUI using **egui/eframe + wgpu**.
2. Retire the TUI. The final product must not depend on ratatui, crossterm,
   Kitty graphics, Sixel, iTerm2 graphics, or half-block rendering.
3. Remove all server-connection modes:
   - remove `cellarium server`;
   - remove `cellarium connect <host>`;
   - remove the SSH connector and remote binary protocol;
   - stop running simulation on tinker while displaying it locally.
4. Run simulation and rendering on the same machine as the GUI.
5. Automatic compute-backend priority is:
   1. an available native discrete-GPU backend, currently NVIDIA CUDA;
   2. an available portable wgpu compute adapter, with mandatory Intel
      integrated-GPU validation and the same path available to Apple, AMD, and
      Raspberry Pi integrated GPUs;
   3. CPU.
6. Settings must let the user select Auto, CUDA, wgpu GPU, or CPU. If an
   explicitly selected backend is unavailable, show the reason instead of
   silently pretending to use it.
7. Preserve the current experiment semantics:
   - one channel by default;
   - one kernel by default for each `(basis polygon, output channel)` binding;
   - additional kernels require an explicit user action;
   - Potential is the raw convolution sum and is not automatically normalized;
   - Growth supports both Rate and Value modes;
   - tilings are strictly edge-to-edge, with no T-junction support;
   - a new Tiling design starts blank, with optional presets or free drawing.
8. The GUI must be mouse-first. Keyboard shortcuts are accelerators and must
   not be the only entry point for primary features.
9. Agentic testing must complete realistic tasks in the real GUI with real
   pointer and keyboard input plus visual screenshot inspection. Unit tests,
   event acknowledgements, traces, and image hashes cannot replace user-level
   visual judgment.

## 2. Repository and branch state

Remote worktree:

```text
/home/wkj/projects/cellarium/.worktrees/basis-workbench-implementation
```

Baseline and relevant commits:

```text
271b79e  v0.2.2 / origin/main
8a41fb4  legacy graphical TUI Workbench design
9599ec1  legacy graphical TUI Workbench plan
5a20955  TUI object strip
f959c00  stable Workbench selection and decision state
4c58cf2  TUI Channels cards and channel-lifecycle fixes
```

Before the GUI decision, the legacy plan was implemented only through Phase A /
Task 3. The RED tests for Task 4 were not retained. The worktree was restored
to committed state before this handoff was written.

Migration value of those commits:

| Commit | Treatment |
| --- | --- |
| `8a41fb4`, `9599ec1` | Keep as a source of user problems and interaction requirements; their technical direction is superseded by this GUI design |
| `5a20955` | Delete TUI layout/rendering; stable object IDs and hit semantics remain useful references |
| `f959c00` | Preserve model-level selection, undo/redo, and decision-transaction ideas |
| `4c58cf2` | Preserve channel IDs, names, and binding freeze/thaw fixes; delete ratatui rendering |

The most recent completed remote gate was:

```sh
cargo fmt
cargo test --locked --lib
cargo test --locked --test workbench_e2e
git diff --check
```

Those tests validate the legacy TUI branch. They are not evidence that the GUI
migration passes.

## 3. Build and test constraints

- The local development machine is a low-performance ARM64 Raspberry Pi. Do
  not run Rust builds on it.
- Run Rust compilation, unit tests, Clippy, and cross-builds on tinker or in
  GitHub Actions.
- The Raspberry Pi should only download prebuilt ARM64 release artifacts after
  verifying their SHA-256 checksums.
- Final agentic acceptance must run that prebuilt artifact in the Raspberry
  Pi's real GUI session.
- tinker may remain the development build machine, but must not be a product
  runtime simulation server.
- Do not infer real GPU performance from a software-Xvfb session.

## 4. What Cellarium is

Cellarium is an editable cellular-automata and continuous-cellular-automata
laboratory. It is not limited to a Conway square grid. Users can define:

- periodic unit cells and tilings;
- multiple basis polygons with independent state semantics inside one unit cell;
- multiple scalar channels on each basis polygon;
- an independent RuleSet for each `(basis, output channel)`;
- one or more kernels in each RuleSet;
- each kernel's source channel, support, weights, sampling metric, and periodic
  offset;
- Rust-like Growth expressions;
- Rate or Value update mode;
- the initial world, colors, visibility, frozen state, and experiment `dt`.

### 4.1 Cardinality rules

Let:

- `B` be the number of basis polygons in the central unit cell;
- `C_active` be the number of non-frozen channels;
- `K(b,c)` be the kernel count for binding `(basis=b, output=c)`.

Then:

- the Growth-binding count is `B × C_active`;
- every binding has an independent Growth program;
- every binding has one kernel by default;
- the number of ordinary Growth inputs exactly equals that binding's kernel
  count;
- the full signature contains `self + K(b,c)`;
- channel count and kernel count are not required to match;
- a kernel may read from any source channel;
- multiple bindings may share a RuleSet and detach through copy-on-write when
  edited.

### 4.2 Growth semantics

```text
fn growth(self: Scalar, k1: Scalar, ..., kN: Scalar) -> Rate | Value
```

- `self` is the current value of the target basis/channel.
- Each `kN` is the raw convolution result of its corresponding kernel.
- Rate mode: `next = clamp(self + dt * result, 0, 1)`.
- Value mode: `next = clamp(result, 0, 1)`.
- The final expression without a trailing semicolon is the result.
- The language supports `let`, `if/else`, arithmetic, comparisons, logical
  operators, and built-in mathematical functions.
- The current language has no `return`, loops, mutable variables, or side
  effects.

### 4.3 Persistence format

Default data directory:

- when `XDG_DATA_HOME` is absolute: `$XDG_DATA_HOME/cellarium/`;
- otherwise: `$HOME/.local/share/cellarium/`.

Files:

- `workbench.ron`: active document, draft document, and revision;
- `experiment.ron`: a self-contained experiment that can be opened and run.

Keep the RON data model compatible. Do not put transient GUI state in experiment
files. Persist window and editor preferences separately in `settings.ron`.

## 5. Current code map

### 5.1 Preserve

| Path | Responsibility |
| --- | --- |
| `src/sim/experiment_model.rs` | ExperimentSpec, Channel, Kernel, Growth, and update mode |
| `src/sim/ruleset.rs` | RuleSet, Binding, shared/default/local-override semantics |
| `src/sim/tiling/**` | Polygons, periodic tilings, validation, coverage, solver, and constraints |
| `src/sim/growth/**` | Lexer, parser, AST, type checker, evaluator, and plot sampling |
| `src/sim/basis_runtime.rs` | Multi-basis compilation and CPU runtime semantics |
| `src/sim/runtime.rs` | Experiment compilation and CPU reference implementation |
| `src/sim/cuda.rs`, `cuda_codegen.rs` | NVIDIA CUDA/NVRTC backend |
| `src/sim/service.rs` | Reusable atomic Apply and active/draft switching logic |
| `src/workbench/history.rs`, `command.rs` | Draft transactions and undo/redo |
| `src/workbench/state.rs` | Existing edit actions and selection semantics; migrate into the GUI Document controller |
| `src/render/camera.rs`, `channels.rs`, `scene_transform.rs` | Reusable math, colors, and coordinate transforms |

### 5.2 Preserve semantics after refactoring, not UI implementation

| Current path | GUI destination |
| --- | --- |
| `src/workbench/tiling_editor.rs` | Extract pure scene/hit/command logic; let an egui Canvas render it |
| `src/workbench/kernel_editor.rs` | Preserve mapping and edit commands; remove RGBA/TUI fake-window assumptions |
| `src/workbench/growth_editor.rs` | Preserve source buffer, diagnostics, and plot model; use an egui text editor |
| `src/workbench/channel_editor.rs` | Preserve ChannelCard view models and lifecycle commands |
| `src/render/basis_scene.rs` | Split into pure geometry scene plus egui/wgpu rendering |
| `src/app.rs` | Split into GUI shell, Document, and SimulationWorker; do not preserve the monolithic terminal event loop |

### 5.3 Delete after feature parity

- `src/tui/**`
- `src/render/display/**`
- `src/remote.rs`
- terminal-specific `src/input.rs`
- `ratatui`, `ratatui-image`, and `crossterm`
- Kitty, Sixel, iTerm2, half-block, and shared-memory graphics
- the SSH connector, remote protocol, and server loop
- PTY and Kitty-protocol tests
- product C/S journeys in `scripts/e2e-tinker.sh`
- `server` and `connect` instructions in README and release documentation

Do not delete these paths before the GUI and local backends reach feature
parity. Keep the branch runnable throughout the migration.

## 6. Target program structure

```text
cellarium
├── gui                 egui application, layout, controls, and canvases
├── document            active/draft state, selection, history, and persistence
├── simulation          local worker, commands, snapshots, and performance metrics
├── sim
│   ├── model           existing Experiment/RuleSet/Tiling/Growth model
│   ├── compile         backend-independent ComputePlan
│   └── backends
│       ├── cuda        NVIDIA
│       ├── wgpu        Intel/Apple/AMD/Raspberry Pi GPUs
│       └── cpu         reference implementation and final fallback
└── persistence         RON workspace/experiment/settings
```

The GUI thread must never execute simulation steps, CUDA compilation, WGSL
pipeline construction, or large readbacks. A SimulationWorker owns the selected
backend, receives Apply, Run, Pause, Step, Reset, and WorldEdit commands, and
publishes its newest displayable state through a latest-only snapshot.

## 7. Target GUI information architecture

Main window:

- top bar: New, Open, Save, Undo, Redo, Apply & Run, Pause/Run, Step, Reset,
  and Backend;
- left navigation: Simulation, Tiling, Channels, Kernels, Growth, Experiment;
- center: the primary canvas or editor for the active section;
- right side: concise object properties and errors, not a wall of shortcuts;
- bottom status: backend, tick, simulation Hz, frame Hz, draft state, and
  persistent errors.

Every primary action must have a visible pointer target and a tooltip.
Shortcuts remain available but belong in Help rather than the Inspector's main
content.

### 7.1 Tiling

- Start blank by default.
- Offer visible preset cards for Square, Triangle, Hexagon, and Octagon+Square.
- Allow freehand polygon construction with the pointer.
- Close a polygon by clicking the first point, double-clicking, or pressing a
  visible Finish button.
- Reject illegal vertices at placement time.
- Emphasize the central unit cell and render its true adjacent copies with
  reduced opacity.
- Provide a visible Solve seams button.
- After solving, edit constrained vertices as a linked system.
- Enforce strict edge-to-edge tiling; do not support T-junctions.

### 7.2 Channels

- Show all channel cards and an Add button at the top.
- Let each card directly select, delete, recolor, show/hide, and freeze its
  channel.
- Make Composite, Solo, and Grid view modes clickable.
- Clearly separate Live state from Draft initial state; never substitute one
  silently.
- Render true polygon geometry rather than shearing a hexagonal lattice into a
  rectangular texture.
- The Inspector must show counts relevant to Channels only.

### 7.3 Kernels

- Show every kernel for the current binding as a card with a thumbnail.
- After Add, make the new card immediately visible and selected.
- Allow direct switching in any order.
- Delete the selected kernel, never an unrelated "last kernel."
- When a referenced kernel is removed, present an understandable decision
  dialog.
- Expose Weights/Support, source/output channel, Affine/World metric, sigma,
  stencil size, and anchor as visible controls.
- Support cell click, drag, wheel fine adjustment, Shift/Ctrl step modifiers,
  and double-click exact numeric entry.
- Use a stable legend for active, negative, zero, inactive, anchor, and selected
  cells.

### 7.4 Growth

- Show the complete function signature at the top of the center panel.
- Make basis, output channel, `self`, and each kernel clickable chips.
- Provide a source editor with a caret, selection, line numbers, syntax
  highlighting, and inline diagnostics.
- Keep the source and plot together in the center panel.
- Default to a curve for zero or one referenced kernel and a heatmap for two or
  more referenced kernels.
- Derive plot dimensions from actual references and user-selected axes, not the
  global kernel count.
- Provide pinned-value controls for non-axis inputs.
- Clearly visualize stale output, no finite samples, and discrete equality
  points.
- Put the syntax reference in a scrollable Help tab on the right.

### 7.5 Experiment

- Summarize basis polygons, active/frozen channels, bindings, current/all
  kernels, Growth programs, `dt`, backend, and diagnostics.
- Make Apply & Run a large, explicit button.
- Apply is a local atomic transaction: replace the active simulation only after
  the new backend is built successfully.
- If Apply fails, preserve the previous active simulation.

## 8. Backend selection and failure semantics

Auto-detection order:

1. the CUDA feature is compiled and the NVIDIA driver, device, and NVRTC are
   available;
2. wgpu finds a non-CPU adapter that satisfies the required compute and storage
   limits:
   - prefer a discrete adapter;
   - then prefer an integrated adapter;
   - validate Intel integrated graphics for every release;
   - permit Apple, AMD, and Raspberry Pi integrated GPUs on the same portable
     path;
3. CPU.

Do not equate the UI renderer with the compute backend. The GUI may render with
wgpu while compute falls back to CPU because the adapter lacks required limits.

Every probe produces a structured report:

```rust
pub struct BackendProbe {
    pub kind: BackendKind,
    pub available: bool,
    pub device_name: Option<String>,
    pub reason: Option<String>,
}
```

When Auto falls back, show one persistent notification, for example:
`CUDA unavailable (NVRTC missing); using Intel Iris Xe via wgpu`.

On a runtime backend error:

- pause the worker;
- retain the last confirmed snapshot;
- try to rebuild on the next backend from that snapshot;
- on success, restore the user's previous running or paused state;
- on failure, remain paused and show the complete error;
- never skip a tick or publish a partially written state.

## 9. Handoff execution order

The next agent must read:

1. this file;
2. `docs/superpowers/specs/2026-08-27-local-egui-gui-migration-design.md`;
3. `docs/superpowers/plans/2026-08-27-local-egui-gui-migration.md`;
4. `docs/feature-inventory.md` for product semantics only; its terminal and
   client/server sections are historical.

Then:

1. create or continue a dedicated GUI branch from this worktree; do not develop
   directly on `origin/main`;
2. confirm that `git status --short` is empty;
3. do not run Cargo on the Raspberry Pi;
4. follow the implementation plan with TDD;
5. keep a runnable local GUI with CPU fallback at every phase;
6. remove TUI and remote code only after GUI feature parity;
7. run the complete agentic GUI journey against the release candidate.

## 10. Status of legacy documents

Keep the following as historical design records, but do not use them to direct
the new implementation:

- `docs/remote-viewer.md`;
- C1 remote-viewer design and plans;
- hybrid remote E2E design and plans;
- Kitty and half-block portions of the legacy visual-Workbench documents;
- terminal and remote capabilities in Sections 12 and 13 of
  `docs/feature-inventory.md`.

If a legacy document conflicts with this handoff or the GUI specification, this
handoff and the GUI specification take precedence.

## 11. Definition of Done

The migration is complete only when all of the following are true:

- the product binary has no `server` or `connect` command;
- Cargo no longer depends on terminal UI or terminal graphics crates;
- the native GUI is the default entry point;
- CUDA, portable wgpu GPU, and CPU backends have consistency tests;
- real hardware validation covers Intel integrated graphics and at least one
  non-Intel integrated GPU;
- a CPU-only machine can start, edit, and Apply & Run an experiment;
- every non-terminal feature in the feature inventory has a visible pointer
  entry point in the GUI;
- full multi-channel, multi-kernel, multi-basis, and multi-Growth journeys pass;
- an experiment survives save, close, and reopen;
- the release candidate passes real pointer, keyboard, and visual agentic
  testing on the Raspberry Pi;
- Windows, macOS, and Linux x86_64/ARM64 artifacts pass startup smoke tests;
- a stable release is published instead of substituting a prerelease for final
  delivery.
