# Local egui/wgpu GUI Migration Design

**Date:** 2026-08-27
**Status:** approved direction, ready for implementation planning
**Product baseline:** Cellarium v0.2.2 plus branch commit `4c58cf2`
**Handoff:** `docs/gui-migration-handoff.md`

## 1. Purpose

Replace the terminal and SSH client/server product with one local native GUI
application. Preserve Cellarium's experiment model and scientific semantics,
while making every ordinary workflow discoverable through graphical,
mouse-first interaction.

The same local process owns:

- the active experiment and editable draft;
- the simulation backend;
- GUI rendering and input;
- persistence and export.

Computation automatically chooses CUDA, portable wgpu compute, or CPU. Remote
simulation is not part of the target product.

## 2. Scope

### 2.1 In scope

- Native desktop GUI with egui/eframe and wgpu renderer.
- Linux x86_64/ARM64, Windows x86_64/ARM64, macOS x86_64/ARM64.
- Local NVIDIA CUDA backend.
- New portable wgpu compute backend, with Intel integrated GPU as a mandatory
  validation target.
- CPU reference/fallback backend.
- GUI equivalents for Simulation, Tiling, Channels, Kernels, Growth, and
  Experiment.
- Local active/draft transaction, undo/redo, Apply & Run, save/open/autosave.
- Real GUI agentic testing through OS input and framebuffer observation.
- Removal of all terminal display and product SSH/server code.

### 2.2 Out of scope

- Web application or browser deployment.
- Multi-user collaboration.
- Remote/headless simulation service.
- Plugin system.
- 3D visualization.
- Automatic arbitrary polygon tiling discovery. The seam solver starts from
  user-provided geometry with plausible complete-edge correspondences.
- T-junction support.
- Zero-copy GPU-to-screen rendering in the first GUI release. A bounded local
  snapshot/readback path is sufficient for 256×256 and keeps all backends
  comparable.

## 3. Technology contract

- Rust edition 2024.
- `eframe = 0.36.1`, using its matching `egui`, `egui-wgpu`, and winit
  dependency family from one lockfile.
- wgpu version must be the same major selected by egui-wgpu; the dependency
  graph may not contain two wgpu major versions.
- eframe features: wgpu, x11, wayland, accesskit, default fonts, persistence,
  and screenshot/inspection support required by test builds.
- CUDA remains optional through the existing `cuda` feature and cudarc.
- RON + serde remain the persistent experiment/workspace format.
- No Tauri, WebView, JavaScript runtime, Qt, Flutter, or browser dependency.
- No ratatui, crossterm, ratatui-image, terminal graphics protocol, or SSH
  runtime dependency in the final release.

If eframe 0.36.1 cannot compile on one required target, the whole egui family
may be moved together to one newer compatible patch/minor. Mixing independent
egui/wgpu versions is forbidden.

## 4. Product principles

1. The main canvas represents the current task, not implementation state.
2. Collections are visible collections. Every Channel, Kernel, basis, and
   Growth input can be selected by mouse.
3. Primary actions have visible controls. Shortcuts only accelerate them.
4. Draft, active simulation, stale plot, invalid program, and backend fallback
   are visually distinct.
5. Changes are reversible. Compound edits are one undo transaction.
6. A failed Apply never destroys the running experiment.
7. GUI responsiveness is independent of simulation throughput.
8. Counts always name their scope.
9. Scientific values are not silently normalized or repaired.
10. Tests complete user goals and inspect visible results.

## 5. Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│ eframe event loop / CellariumGui                            │
│  AppShell ─ Navigation ─ Editors ─ Dialogs ─ Status         │
│       │                 │                  │                 │
│       └──────────── DocumentController ────┘                 │
│                      active + draft + history               │
│                               │ ApplyCommand                │
│                               ▼                             │
│                     SimulationController                    │
│                 commands + latest-only snapshot             │
│                               │                             │
│                      SimulationWorker                       │
│               BackendSelector + LocalBackend                │
│                   ╱           │           ╲                 │
│                CUDA          wgpu          CPU               │
└─────────────────────────────────────────────────────────────┘
```

### 5.1 Thread ownership

GUI thread owns:

- eframe/egui context;
- transient view state;
- DocumentController;
- textures generated from the latest immutable snapshot;
- user input dispatch.

Simulation worker owns:

- current LocalBackend;
- backend-resident state;
- running/paused state;
- step scheduler;
- backend probes and runtime recovery.

The GUI thread never blocks waiting for a simulation step, CUDA/NVRTC compile,
WGSL pipeline build, or GPU readback.

### 5.2 Command flow

```rust
pub enum SimulationCommand {
    Apply {
        request_id: u64,
        experiment: Box<ExperimentSpec>,
        initial: InitialStatePolicy,
    },
    SetRunning(bool),
    Step(u32),
    Reset,
    Randomize { seed: u64 },
    Clear,
    EditWorld(Vec<WorldEdit>),
    SelectBackend(BackendPolicy),
    Shutdown,
}
```

Commands that represent user intent are never silently dropped. Pointer paint
events may be coalesced into one ordered `EditWorld` batch per GUI frame.

### 5.3 Snapshot flow

```rust
pub struct SimulationSnapshot {
    pub generation: u64,
    pub revision: u64,
    pub tick: u64,
    pub running: bool,
    pub backend: BackendDescriptor,
    pub layout: StateLayout,
    pub cells: std::sync::Arc<[f32]>,
    pub step_stats: StepStats,
    pub error: Option<RuntimeNotice>,
}
```

Snapshots use a latest-only `Arc<RwLock<Arc<SimulationSnapshot>>>`. Publishing
a new snapshot replaces the old pointer atomically from the consumer's
perspective. GUI rendering may skip intermediate snapshots but always displays
a self-consistent layout/cell pair.

Snapshots are published:

- after Apply, Reset, Randomize, Clear, Step, or WorldEdit;
- while running, at a configurable display cadence capped at 60 Hz;
- immediately on backend transition or error.

Backend steps may run faster than display cadence.

## 6. Document and transaction model

```rust
pub struct DocumentController {
    pub active: ExperimentSpec,
    pub draft: ExperimentSpec,
    pub active_revision: u64,
    pub status: DraftStatus,
    pub selection: EditorSelection,
    pub history: History,
    pub conflict: Option<DocumentConflict>,
}
```

The existing WorkbenchState edit operations move behind this controller.
Geometry, Channel, Kernel, Growth, and Experiment editors dispatch typed
DocumentCommand values. They do not mutate `ExperimentSpec` directly.

Apply procedure:

1. clone and validate the complete draft;
2. normalize RuleSets;
3. compile a backend-neutral ComputePlan;
4. ask SimulationWorker to build a candidate backend without replacing active;
5. on success, atomically install candidate, increment revision, mark draft
   clean, start running, autosave;
6. on failure, keep active backend/world/revision and show diagnostics linked
   to document paths.

There is no remote revision conflict. Revisions remain useful for persistence,
stale asynchronous Apply responses, and tests.

Undo/redo snapshots include stable selected ChannelId, BasisId, RuleSetId,
KernelId, prototype and plot axes. Selection is normalized only after the
model transaction succeeds.

## 7. Backend-neutral compilation

Current CUDA and CPU paths compile overlapping model representations. The
migration introduces one `ComputePlan` that fixes ordering and semantics
before a backend is selected:

```rust
pub struct ComputePlan {
    pub width: u32,
    pub height: u32,
    pub bases: Vec<BasisId>,
    pub channels: Vec<CompiledChannel>,
    pub bindings: Vec<CompiledBinding>,
    pub kernels: Vec<CompiledKernel>,
    pub growth: Vec<TypedGrowthProgram>,
    pub boundary: CompiledBoundary,
    pub dt: f32,
}
```

The plan contains:

- stable basis/channel/kernel IDs and dense backend indices;
- flattened periodic/raster topology;
- source basis/channel for every kernel sample;
- raw weights and support masks;
- typed Growth AST and external symbol ordering;
- UpdateMode;
- explicit boundary behavior.

CPU, CUDA, and WGSL generation consume the same plan. The CPU backend is the
semantic reference.

## 8. Local backend interface

```rust
pub trait LocalBackend: Send {
    fn descriptor(&self) -> &BackendDescriptor;
    fn tick(&self) -> u64;
    fn set_running_state(&mut self, state: &WorldSnapshot) -> Result<(), BackendFailure>;
    fn apply_edits(&mut self, edits: &[WorldEdit]) -> Result<(), BackendFailure>;
    fn step(&mut self, steps: u32) -> Result<StepStats, BackendFailure>;
    fn readback(&mut self) -> Result<WorldSnapshot, BackendFailure>;
}
```

The backend owns its working state. GPU state is not copied host→device and
device→host for every simulation step. Readback occurs only for snapshots,
edits, Apply transitions, and recovery.

### 8.1 Backend kinds

```rust
pub enum BackendKind {
    Cuda,
    Wgpu,
    Cpu,
}

pub enum BackendPolicy {
    Auto,
    RequireCuda,
    RequireWgpu { adapter: Option<AdapterKey> },
    RequireCpu,
}
```

### 8.2 Auto selection

Auto candidates:

1. CUDA if compiled and driver/device/NVRTC/required feature probes pass.
2. wgpu compute adapters satisfying ComputePlan limits, sorted:
   - discrete GPU;
   - integrated GPU;
   - virtual GPU.
   CPU adapters are not counted as the wgpu GPU fallback.
3. CPU.

Intel integrated graphics is a mandatory validation target. The implementation
is vendor-neutral and also accepts Apple, AMD, and Raspberry Pi integrated
adapters.

Every probe yields:

```rust
pub struct BackendProbe {
    pub kind: BackendKind,
    pub available: bool,
    pub device_name: Option<String>,
    pub device_type: Option<GpuDeviceType>,
    pub reason: Option<String>,
}
```

Settings displays all probes. Auto fallback shows a persistent notification
with the rejected and selected devices.

### 8.3 Runtime failure

On a recoverable backend error:

1. stop scheduling steps;
2. read back or use the last confirmed snapshot;
3. mark the failing candidate unavailable for this session;
4. construct the next Auto backend from the same ComputePlan and snapshot;
5. publish a backend-transition snapshot;
6. restore the user's running/paused state.

If state readback itself fails, use the last confirmed snapshot and clearly
report that at most the unconfirmed in-flight step was discarded.

An explicit Require policy never silently changes kind. It pauses and asks the
user to select Auto/another backend.

## 9. wgpu compute design

### 9.1 Why it is required

wgpu as eframe's renderer does not automatically accelerate Cellarium's
simulation. A dedicated compute backend must:

- allocate storage buffers for current/next state, kernels and topology;
- generate valid WGSL from TypedGrowthProgram;
- create compute pipelines;
- dispatch one cell/basis/channel work item;
- enforce identical Rate/Value and boundary semantics;
- provide staging readback at snapshot cadence.

### 9.2 WGSL code generation

Add a backend-neutral expression emitter over the typed Growth AST. It emits
only the already validated language:

- scalar literals and external symbols;
- arithmetic, comparison and boolean operators;
- let bindings;
- if/else expressions;
- approved built-ins.

No source string is concatenated directly into WGSL without AST emission.
Generated source includes document paths in comments for diagnostics but does
not rely on user identifiers as WGSL identifiers; symbols map to dense slots.

### 9.3 Dispatch

Initial implementation uses one pipeline per distinct RuleSet shape and a
dispatch over `width × height × bindings`. Read-only kernel/topology data is
shared. Current and next state are double-buffered.

Required device limits are computed before requesting the device. A plan too
large for an adapter produces an unavailable probe with the exact limit, then
Auto tries CPU.

### 9.4 Correctness

For each fixture, compare CPU, CUDA when available, and wgpu:

- Conway;
- Lenia/Orbium;
- non-normalized kernel;
- multiple source channels;
- frozen channel;
- two-basis triangle tiling;
- regular hexagonal periodic kernel;
- Rate and Value;
- boundary modes;
- non-finite Growth rejection.

Tolerance is `1e-5` per scalar after one step and `1e-4` after 100 steps,
unless a fixture documents a stricter exact discrete comparison.

## 10. GUI shell

### 10.1 Window layout

```text
┌──────────────────────────────────────────────────────────────────┐
│ File  Save  Undo  Redo  Apply & Run  ▶/Ⅱ  Step  Reset  Backend  │
├────────────┬──────────────────────────────────┬──────────────────┤
│ Simulation │                                  │ Inspector        │
│ Tiling     │          Main workspace          │ object + errors  │
│ Channels   │                                  │ Help tab         │
│ Kernels    │                                  │                  │
│ Growth     │                                  │                  │
│ Experiment │                                  │                  │
├────────────┴──────────────────────────────────┴──────────────────┤
│ Clean/Dirty · tick · sim Hz · frame Hz · backend · notice       │
└──────────────────────────────────────────────────────────────────┘
```

- Sidebar is resizable and collapsible.
- Inspector is resizable, has Properties and Help tabs.
- Main workspace uses remaining area and never draws under side panels.
- All icon-only buttons have accessible names and hover tooltips.
- Destructive actions require a dialog unless fully reversible and
  unambiguous.
- Global errors persist until dismissed or resolved.

### 10.2 Theme

- Board interior background: pure black.
- Domain exterior: existing deep blue.
- One channel: high-contrast light neutral.
- Three channels: RGB.
- Positive kernel: cyan; negative: red; active zero: dark neutral;
  inactive: outline only; anchor: gold; selection: white.
- Draft: amber accent; active/live: green/cyan; invalid: red; stale: amber.
- Color is never the only state indicator; icon/shape/text accompanies it.

### 10.3 Camera and pointer coordinates

Every canvas uses one explicit transform:

```rust
pub struct CanvasTransform {
    pub viewport: egui::Rect,
    pub center_world: [f64; 2],
    pub pixels_per_world: f64,
}
```

`world_to_screen` and `screen_to_world` are exact inverses within float
tolerance. Rendering and hit testing consume the same transform instance for
the same frame. Pointer-centered zoom keeps the world point under the pointer
fixed. This prevents the historical paint/select coordinate drift.

## 11. Section designs

### 11.1 Simulation

- Live simulation canvas fills center.
- Toolbar exposes run/pause, step, reset, randomize, clear, fit and channel
  view.
- Left paint, right erase, middle pan, wheel zoom.
- Brush radius/value have visible controls.
- Hover inspector reports basis, channel values and world coordinate.
- Backend transition never blanks or shrinks the last valid image.

### 11.2 Tiling

- Empty state presents visible preset cards and Draw from scratch.
- Drawing mode shows vertices, segments, preview edge, Finish and Cancel.
- Invalid candidate point is rejected before insertion with local reason.
- Click first vertex, double-click or Finish closes the polygon.
- Center unit cell is strong; one actual adjacency ring is translucent.
- Neighbor placement comes from translation vectors and seam relations, never
  a hard-coded rectangular grid.
- Solve seams proposes complete-edge correspondences only.
- After solve, constrained vertex drag updates its equivalence class and
  lattice vectors as one command.

### 11.3 Channels

- Stable-ID card strip with color, eye, active/frozen and delete.
- Add is a trailing card.
- Composite/Solo/Grid are tabs.
- Source tabs are Live and Draft initial.
- If structures differ, Draft is default, labelled not applied and fitted;
  Live explicitly shows old structure.
- A visible Apply & Run button resolves the mismatch.
- Inspector shows only all/active/frozen/selected/color/view.

### 11.4 Kernels

- Binding selector exposes basis and output channel.
- Every kernel has a thumbnail card, symbol, source and delete.
- Add immediately selects the new card.
- Clicking any card updates the editor and Growth signature.
- Tool palette: Weights, Support, Gaussian, metric, sigma, stencil/anchor,
  source channel, output Binding and Reset RuleSet.
- Wheel over active cell edits by 0.05; Shift 0.005; Ctrl 0.5.
- Double-click/Enter opens exact numeric popover.
- Wheel over empty/inactive space zooms; middle drag pans.
- Delete referenced kernel offers Replace references with 0 and remove, or
  Cancel. The source rewrite and deletion are one transaction.

### 11.5 Growth

- Basis/output selector and complete signature are always visible.
- Kernel chips use the same identity/color as Kernel cards.
- Clicking a chip navigates to that kernel.
- Multiline TextEdit uses a custom lexer layouter and gutter for line numbers.
- Diagnostics underline exact spans and list concise messages below source.
- Curve/heatmap is a real vector/GPU plot, not character art.
- Default axes derive from referenced external symbols.
- User can assign X/Y by clicking chips; remaining inputs have pinned sliders
  and exact entry.
- Plot min/max are editable.
- Stale plot retains the last valid data with explicit overlay.
- Equality discontinuities show sampled markers.
- Help tab documents signature, Rate/Value, syntax and built-ins.

### 11.6 Experiment

- Summary cards show world, tiling/basis, channels, Bindings, selected and
  total kernels, Growth and dt.
- Diagnostics navigate to the relevant section/object.
- Apply & Run is primary.
- Save Draft and Revert are secondary.
- Backend panel shows Auto order and probe reasons.

## 12. Persistence

Keep existing workspace and experiment RON compatibility. Add:

```rust
pub struct GuiSettings {
    pub backend_policy: BackendPolicy,
    pub window: WindowSettings,
    pub theme: ThemeChoice,
    pub autosave_seconds: u32,
}
```

Settings path is `$XDG_CONFIG_HOME/cellarium/settings.ron`, or
`$HOME/.config/cellarium/settings.ron`.

Experiment/workspace saves remain atomic: temporary sibling, flush/sync,
rename, Unix 0600 for private user data. Autosave never blocks the GUI thread;
it saves an immutable document snapshot on a worker.

On first GUI launch, existing v0.2.2 `workbench.ron` and `experiment.ron`
are imported. No Channel, Kernel, RuleSet, or Growth is silently removed.

## 13. CLI and process behavior

Final CLI:

```text
cellarium
cellarium --experiment PATH
cellarium --backend auto|cuda|wgpu|cpu
cellarium --safe-mode
cellarium --version
```

`--safe-mode` selects CPU compute, disables autosave recovery mutations, and
starts with conservative renderer options for diagnosis.

`server`, `connect`, and `--ssh-command` are hard errors with:

```text
remote/server mode was removed; Cellarium now runs simulation locally
```

No background child process survives normal exit. Closing the window sends
Shutdown, waits for a bounded worker join, flushes pending autosave, and exits.

## 14. Performance targets

Targets are measured on Release builds:

- GUI input-to-visible-control feedback p95 < 50 ms while simulation runs.
- Pointer pan/zoom/edit p95 < 50 ms.
- GUI maintains 30 visual FPS on the supported low-end target when the window
  is active and content changes.
- Simulation cadence is independent and may be lower.
- Snapshot publishing never queues more than one unread image.
- No user action waits for more than one frame without busy indication.
- CUDA and wgpu state remain device-resident between steps.
- CPU backend uses parallel iteration only after correctness; the first
  migration may retain the current reference implementation.

The Raspberry Pi is a functional low-end target. Its throughput is reported,
not compared against tinker or used to weaken correctness.

## 15. Testing strategy

### 15.1 Pure tests

- DocumentCommand transactions and selection.
- CanvasTransform inverse/property tests.
- scene geometry and hit tests.
- Growth lexer/typecheck/plot.
- tiling solve/coverage.
- backend selection/probe ordering.
- CPU/CUDA/wgpu numerical parity.
- persistence round trips and old-format import.

### 15.2 Headless GUI tests

Use egui_kittest/AccessKit test support for:

- every visible primary control has a stable ID and accessible name;
- click targets invoke the same DocumentCommand as shortcuts;
- resizing never overlaps panels;
- object collections remain scrollable/selectable;
- dialogs expose buttons and focus;
- screenshots for representative 1280×720, 1920×1080 and narrow layouts.

These tests support, but do not replace, real GUI tests.

### 15.3 Real agentic tests

Run the exact packaged Release in a clean user data directory. The Agent:

1. observes a screenshot;
2. selects coordinates from visible UI;
3. sends real OS mouse/keyboard events;
4. observes the new screenshot;
5. records semantic result and UX defects;
6. adapts subsequent actions.

Mandatory journey:

1. launch default and identify backend/fallback;
2. pause/run/step/reset/randomize/clear;
3. paint, erase, pan, zoom and inspect Simulation;
4. start blank Tiling, draw and close a triangle, undo during drawing;
5. choose hex preset and verify non-orthogonal neighbor ring;
6. create an imperfect multi-polygon cell, solve seams, constrained-drag;
7. add channels to three, verify RGB, select, solo/grid/composite, color,
   hide, freeze, delete, undo/redo;
8. for one Binding add at least three kernels, switch in nonsequential order,
   make each visually distinct, edit support/value/exact value/source/metric,
   delete a referenced kernel through both cancel and resolve paths;
9. verify Growth signature arity, edit valid and invalid programs, curve,
   heatmap, axis chips, pinned input and Rate/Value;
10. Apply & Run and observe tick/state evolution using the designed geometry;
11. force CPU, return Auto, and inspect backend notice;
12. save, close, reopen, verify design and selections;
13. resize repeatedly and repeat pointer edits without coordinate drift,
   blank frames, stale overlays or freezes.

Every row needs before/after images, action, expected result, observed result,
PASS/FAIL and UX notes. An unexplained interface is a failure even if the
underlying mutation is correct.

## 16. CI and release

CI stages:

1. fmt, clippy, unit/integration tests;
2. CPU-only build and tests;
3. Linux wgpu software/headless validation;
4. CUDA parity on a CUDA runner when available;
5. GUI startup/screenshot smoke under Linux Xvfb;
6. cross-platform release builds;
7. package smoke on each architecture;
8. checksum generation.

Release packages:

- Linux x86_64 and ARM64 tar.gz;
- Windows x86_64 and ARM64 zip;
- macOS x86_64 and ARM64 tar.gz or app bundle archive.

Linux documentation lists X11/Wayland and Mesa/Vulkan runtime requirements.
No stable tag is published until the exact candidate artifacts pass the real
agentic journey. Final delivery is a normal stable GitHub Release, not a
prerelease.

## 17. Migration sequence

1. Add GUI shell beside the old TUI; default remains old until shell starts.
2. Extract DocumentController and local SimulationWorker.
3. Add backend-neutral ComputePlan and portable wgpu compute.
4. Implement Simulation GUI.
5. Implement Tiling, Channels, Kernels, Growth, Experiment GUI.
6. Make GUI the default while keeping TUI code compile-only for one checkpoint.
7. Pass feature parity and real agentic journey locally.
8. Delete TUI, terminal render, remote protocol, server/connect and old tests.
9. Rewrite docs/CI/package.
10. Run final cross-platform and agentic release gates.

At every checkpoint, `cargo test --locked --all-targets` and the CPU-only
variant must pass on tinker/CI. The tree may not spend multiple tasks with no
launchable product path.

## 18. Acceptance criteria

- Default command opens a native GUI.
- No terminal UI or remote/server product mode remains.
- All non-terminal features in `docs/feature-inventory.md` have GUI entry.
- GUI stays responsive during compilation, simulation and readback.
- Auto backend visibly follows CUDA → portable GPU → CPU.
- Intel integrated GPU path is actually exercised.
- CPU fallback produces the same semantics.
- Tiling, Channel, Kernel, Growth and Experiment full journeys pass.
- Multi-kernel add/select/edit/delete is mouse-complete.
- Growth signature, axes and plot match the selected Binding.
- Save/reopen preserves user designs.
- No coordinate drift, transient stale frame, layer leak or unrecoverable
  freeze occurs during the resize/edit stress journey.
- Exact packaged stable artifacts pass release smoke and agentic validation.
