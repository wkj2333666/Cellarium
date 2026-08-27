# Local egui/wgpu GUI Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking. Execute inline; do not dispatch
> subagents because the user explicitly prohibited subagents in this project.

**Goal:** Replace Cellarium's TUI and SSH client/server product with a local
native egui/wgpu GUI using automatic CUDA → portable GPU → CPU compute
fallback.

**Architecture:** Preserve ExperimentSpec, RuleSet, tiling, Growth, CPU and
CUDA semantics. Introduce a GUI-independent DocumentController, a local
SimulationWorker, a backend-neutral ComputePlan and a portable wgpu compute
backend. Implement every editor as an egui panel/canvas, verify feature parity,
then delete terminal and remote code.

**Tech Stack:** Rust 2024, eframe/egui 0.36.1, matching egui-wgpu/wgpu family,
cudarc CUDA, serde/RON, AccessKit/egui_kittest, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-08-27-local-egui-gui-migration-design.md`

## Global Constraints

- Do not run Cargo, rustc, Clippy or linking on the local Raspberry Pi.
- Run Rust builds/tests on tinker or GitHub Actions.
- Raspberry Pi agentic testing uses only checksum-verified precompiled ARM64
  Release artifacts.
- The product has no server, connect, SSH or remote simulation mode.
- Auto compute order is CUDA, compatible non-CPU wgpu adapter, then CPU.
- Intel integrated GPU is a mandatory wgpu validation target; the wgpu path is
  vendor-neutral.
- Default Channel count is one; default Kernel count per Binding is one.
- Potential remains an unnormalized raw convolution result.
- Tilings are strict edge-to-edge; T-junctions are rejected.
- GUI is mouse-first; shortcuts are secondary accelerators.
- Active simulation is replaced only after a complete candidate validates and
  constructs successfully.
- All model edits are typed, undoable DocumentCommand transactions.
- Every task follows RED → GREEN → refactor and ends in a focused commit.
- Keep a launchable path at every checkpoint; delete legacy only after GUI
  parity.
- Do not publish a prerelease as the final deliverable.

## File Structure

### New GUI and document modules

- `src/gui/mod.rs`: GUI public entry and module routing.
- `src/gui/run.rs`: eframe startup and native options.
- `src/gui/app.rs`: thin CellariumGui composition root.
- `src/gui/theme.rs`: colors, typography and spacing.
- `src/gui/layout.rs`: top/sidebar/center/inspector/status shell.
- `src/gui/widgets/*.rs`: object cards, numeric popovers, decision dialogs,
  notices and accessible controls.
- `src/gui/canvas/transform.rs`: shared screen/world transform.
- `src/gui/canvas/world.rs`: Simulation state rendering/hit testing.
- `src/gui/canvas/tiling.rs`: polygon/neighbor/seam scene.
- `src/gui/canvas/channels.rs`: live/draft channel scene.
- `src/gui/canvas/kernel.rs`: raster and periodic kernel scene.
- `src/gui/canvas/growth.rs`: curve/heatmap rendering.
- `src/gui/sections/*.rs`: one focused section controller/view each.
- `src/document/mod.rs`: DocumentController and public commands.
- `src/document/selection.rs`: stable editor selection.
- `src/document/persistence.rs`: workspace/experiment/settings I/O.

### New local simulation modules

- `src/sim/compute_plan.rs`: backend-neutral dense compiled plan.
- `src/sim/local_backend.rs`: LocalBackend trait and common snapshots/errors.
- `src/sim/backend_selector.rs`: probes, policy and fallback order.
- `src/sim/worker.rs`: command loop, scheduler and latest snapshot.
- `src/sim/wgpu_backend.rs`: portable compute backend.
- `src/sim/wgsl_codegen.rs`: typed Growth AST → WGSL.

### Reused/refactored

- `src/sim/experiment_model.rs`, `ruleset.rs`, `tiling/**`,
  `growth/**`, `basis_runtime.rs`.
- `src/sim/runtime.rs`: CPU reference backend consumes ComputePlan.
- `src/sim/cuda.rs`, `cuda_codegen.rs`: CUDA consumes ComputePlan and owns
  device state between steps.
- `src/workbench/history.rs`, `command.rs`, `state.rs`: migrate model
  logic into DocumentController.
- `src/render/camera.rs`, `channels.rs`, `scene_transform.rs`: retain pure
  math; remove terminal assumptions.

### Deleted at final parity gate

- `src/tui/**`, `src/render/display/**`, `src/remote.rs`.
- Terminal-only `src/input.rs` and giant old event loop in `src/app.rs`.
- Remote/PTY/Kitty tests and product scripts.
- ratatui, ratatui-image, crossterm and terminal graphics dependencies.

---

## Phase A — GUI shell and local state boundaries

### Task 1: Add a launchable egui shell beside the legacy path

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `src/gui/mod.rs`
- Create: `src/gui/run.rs`
- Create: `src/gui/app.rs`
- Create: `src/gui/theme.rs`
- Create: `src/gui/layout.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/gui_startup.rs`

**Interfaces:**
- Produces: `gui::run(GuiLaunchOptions) -> Result<(), GuiStartupError>`.
- Produces: temporary `--gui` developer flag; final task makes GUI default
  and removes the flag.

- [ ] **Step 1: Write the failing CLI and GUI-construction tests**

```rust
#[test]
fn gui_flag_selects_local_gui_without_server_or_host() {
    let options = parse_cli([OsString::from("--gui")]).unwrap();
    assert_eq!(options.mode, CliMode::Gui);
}

#[test]
fn gui_model_constructs_without_opening_a_window() {
    let model = CellariumGui::for_test(ExperimentSpec::single_channel_lenia(8, 8));
    assert_eq!(model.navigation().selected(), Section::Simulation);
}
```

- [ ] **Step 2: Run RED remotely**

Run:

```sh
cargo test --locked --test gui_startup
```

Expected: compile failure because `gui`, `CliMode::Gui`, and
`CellariumGui` do not exist.

- [ ] **Step 3: Add one coherent egui/wgpu dependency family**

Add eframe 0.36.1 with wgpu, x11, wayland, accesskit, default_fonts and
persistence. Add matching egui_kittest as a dev dependency. Add direct wgpu,
bytemuck and pollster versions compatible with egui-wgpu. Run
`cargo tree -d | rg 'egui|wgpu|winit'` and reject duplicate major versions.

- [ ] **Step 4: Implement a static three-panel shell**

`CellariumGui::update` renders top actions, six navigation items, an empty
center panel, resizable Inspector and status bar. Every button uses a stable
egui ID, visible label or accessible name, and tooltip.

- [ ] **Step 5: Add a headless UI snapshot test**

Use egui_kittest to render 1280×720. Assert all six sections and top-level
Save, Undo, Redo, Apply & Run, Run/Pause, Step, Reset and Backend controls are
present and clickable.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo fmt --all
cargo test --locked --test gui_startup
cargo test --locked --lib
```

Commit:

```sh
git add Cargo.toml Cargo.lock src/gui src/lib.rs src/main.rs tests/gui_startup.rs
git commit -m "feat: add native egui application shell"
```

### Task 2: Extract a GUI-independent DocumentController

**Files:**
- Create: `src/document/mod.rs`
- Create: `src/document/selection.rs`
- Modify: `src/lib.rs`
- Refactor: `src/workbench/state.rs`
- Refactor: `src/workbench/command.rs`
- Refactor: `src/workbench/history.rs`
- Test: `src/document/mod.rs`

**Interfaces:**
- Produces: `DocumentController`, `DocumentCommand`, `EditorSelection`,
  `DocumentChange`, `ApplyCandidate`.
- Consumes: `ExperimentSpec`, stable model IDs and existing History.

- [ ] **Step 1: Write failing transaction tests**

```rust
#[test]
fn delete_add_undo_redo_preserves_stable_channel_selection() {
    let mut doc = DocumentController::new(three_channel_spec());
    doc.select_channel(ChannelId(1)).unwrap();
    doc.execute(DocumentCommand::DeleteSelectedChannel).unwrap();
    assert_eq!(doc.selection().channel, ChannelId(2));
    doc.undo().unwrap();
    assert_eq!(doc.selection().channel, ChannelId(1));
    doc.redo().unwrap();
    assert_eq!(doc.selection().channel, ChannelId(2));
}

#[test]
fn failed_command_changes_neither_draft_history_nor_selection() {
    let mut doc = DocumentController::new(one_channel_spec());
    let before = doc.audit_snapshot();
    assert!(doc.execute(DocumentCommand::DeleteSelectedChannel).is_err());
    assert_eq!(doc.audit_snapshot(), before);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib document::tests`

- [ ] **Step 3: Define typed commands**

Include selection, Channel lifecycle, polygon construction/vertex edits,
Kernel add/delete/value/support/source/metric, Growth source/mode/axes and
Experiment dt. A command returns `DocumentChange { generation, affected }`.

- [ ] **Step 4: Move reusable Workbench model logic**

Move state mutations without importing egui, ratatui, terminal events or
screen rectangles. Keep temporary adapters so legacy tests compile.

- [ ] **Step 5: Add Apply candidate isolation**

`prepare_apply()` clones, validates, normalizes and compiles the draft. It
does not mutate active or clear history. `accept_apply(request_id, spec)`
updates active only for the latest pending request.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --lib document
cargo test --locked --lib workbench
```

Commit:

```sh
git add src/document src/workbench src/lib.rs
git commit -m "refactor: extract local experiment document controller"
```

### Task 3: Compile one backend-neutral ComputePlan

**Files:**
- Create: `src/sim/compute_plan.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/sim/runtime.rs`
- Modify: `src/sim/basis_runtime.rs`
- Test: `src/sim/compute_plan.rs`

**Interfaces:**
- Produces: `compile_compute_plan(&ExperimentSpec) -> Result<ComputePlan,
  Vec<Diagnostic>>`.
- Produces dense `CompiledBinding`, `CompiledKernel`,
  `TypedGrowthProgram` ordering consumed by all backends.

- [ ] **Step 1: Write failing plan invariants**

```rust
#[test]
fn plan_preserves_basis_channel_binding_and_kernel_order() {
    let spec = two_basis_three_channel_multi_kernel_spec();
    let plan = compile_compute_plan(&spec).unwrap();
    assert_eq!(plan.bases.len(), 2);
    assert_eq!(plan.channels.len(), 3);
    assert_eq!(plan.bindings.len(), 6);
    assert_eq!(plan.binding(BasisId(1), ChannelId(2)).kernels.len(), 3);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib sim::compute_plan::tests`

- [ ] **Step 3: Flatten model IDs without changing semantics**

Build dense maps once. Store raw weights, support, boundary constants,
source/target indices, periodic basis offsets, UpdateMode, dt and typed Growth
AST. Reject missing/duplicate bindings and non-finite data with document paths.

- [ ] **Step 4: Make CPU runtime consume ComputePlan**

Delete duplicate ordering in `compile_experiment` only after one-step outputs
for all existing fixtures are unchanged.

- [ ] **Step 5: Add serialization-free audit output**

Expose `ComputePlanSummary` for UI/tests: dimensions, bases, channels,
bindings, effective kernels and estimated buffer bytes.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --lib sim::compute_plan
cargo test --locked --lib sim::runtime
```

Commit:

```sh
git add src/sim/compute_plan.rs src/sim/mod.rs src/sim/runtime.rs src/sim/basis_runtime.rs
git commit -m "refactor: compile experiments into a shared compute plan"
```

### Task 4: Add LocalBackend and a non-blocking SimulationWorker

**Files:**
- Create: `src/sim/local_backend.rs`
- Create: `src/sim/worker.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/gui/app.rs`
- Test: `src/sim/worker.rs`

**Interfaces:**
- Produces: `LocalBackend`, `SimulationCommand`, `SimulationSnapshot`,
  `SimulationController`.
- Consumes: `ComputePlan` and CPU backend initially.

- [ ] **Step 1: Write failing responsiveness and ordering tests**

```rust
#[test]
fn worker_acks_pause_before_scheduling_another_step() {
    let fake = BlockingBackend::new();
    let controller = SimulationController::spawn(Box::new(fake.clone()));
    controller.send(SimulationCommand::SetRunning(true)).unwrap();
    fake.wait_for_step_start();
    controller.send(SimulationCommand::SetRunning(false)).unwrap();
    fake.finish_step();
    let snapshot = controller.wait_for(|s| !s.running);
    assert_eq!(fake.steps_started(), 1);
    assert!(!snapshot.running);
}

#[test]
fn unread_snapshots_are_replaced_not_queued() {
    let controller = fast_fixture();
    controller.step(100);
    assert_eq!(controller.snapshot_slot_depth(), 1);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib sim::worker::tests`

- [ ] **Step 3: Implement worker ownership**

Use a dedicated named thread. It owns backend and scheduling. Commands are
ordered; paint events are batched by GUI before send. Snapshots replace one
Arc slot. Shutdown closes input and joins with a bounded visible exit path.

- [ ] **Step 4: Implement atomic local Apply**

Build the candidate backend while the active backend remains available.
Install only after complete construction and initial-state upload. Reply with
accepted revision or path diagnostics.

- [ ] **Step 5: Connect shell status without blocking**

CellariumGui polls the snapshot Arc, displays tick/backend/running, and calls
`ctx.request_repaint_after`. It never waits on worker replies inside update.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --lib sim::worker
cargo test --locked --test gui_startup
```

Commit:

```sh
git add src/sim/local_backend.rs src/sim/worker.rs src/sim/mod.rs src/gui/app.rs
git commit -m "feat: run local simulation outside the GUI thread"
```

---

## Phase B — Portable compute and fallback

### Task 5: Generate WGSL from typed Growth programs

**Files:**
- Create: `src/sim/wgsl_codegen.rs`
- Modify: `src/sim/growth/ast.rs`
- Modify: `src/sim/growth/types.rs`
- Test: `src/sim/wgsl_codegen.rs`

**Interfaces:**
- Produces: `generate_wgsl(&ComputePlan) -> Result<GeneratedWgsl,
  BackendFailure>`.
- `GeneratedWgsl` contains source, entry points and dense symbol maps.

- [ ] **Step 1: Write exact WGSL generation tests**

```rust
#[test]
fn rate_value_branches_emit_typed_wgsl_without_user_identifiers() {
    let plan = plan_with_source("let x = k1 * 2; if x > 1 { -self } else { x }");
    let wgsl = generate_wgsl(&plan).unwrap();
    assert!(wgsl.source.contains("fn growth_0"));
    assert!(wgsl.source.contains("select(") || wgsl.source.contains("if ("));
    assert!(!wgsl.source.contains("let x ="));
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib sim::wgsl_codegen::tests`

- [ ] **Step 3: Emit the complete validated language**

Implement constants, dense external slots, unary/binary operators, let,
if/else, comparisons, booleans and every documented built-in. Reject any AST
variant lacking WGSL semantics before pipeline creation.

- [ ] **Step 4: Add shader validation tests**

Use wgpu/naga validation in tests without requesting a physical adapter.
Compile every Growth fixture and diagnostic span.

- [ ] **Step 5: Verify and commit**

Run:

```sh
cargo test --locked --lib wgsl_codegen
```

Commit:

```sh
git add src/sim/wgsl_codegen.rs src/sim/growth
git commit -m "feat: generate portable compute shaders from growth programs"
```

### Task 6: Implement the wgpu compute backend

**Files:**
- Create: `src/sim/wgpu_backend.rs`
- Modify: `src/sim/local_backend.rs`
- Modify: `src/sim/mod.rs`
- Test: `src/sim/wgpu_backend.rs`
- Test: `tests/backend_parity.rs`

**Interfaces:**
- Produces: `WgpuExperimentBackend::probe(instance, plan)` and
  `new(adapter, plan, state)`.
- Implements `LocalBackend`.

- [ ] **Step 1: Write adapter-free layout and dispatch tests**

```rust
#[test]
fn buffer_layout_matches_cpu_state_layout() {
    let plan = two_basis_three_channel_plan();
    let layout = WgpuBufferLayout::for_plan(&plan).unwrap();
    assert_eq!(layout.state_scalars, plan.width as usize * plan.height as usize * 2 * 3);
    assert_eq!(layout.workgroups[2], plan.bindings.len() as u32);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib wgpu_backend`

- [ ] **Step 3: Allocate persistent GPU state**

Create current/next storage buffers, immutable topology/kernel buffers,
uniform metadata, pipelines and staging readback. Keep state on device across
steps; swap buffers after successful dispatch.

- [ ] **Step 4: Implement edits and snapshots**

WorldEdit uploads only changed ranges where practical. Readback occurs only on
snapshot demand and waits on the worker, never GUI thread.

- [ ] **Step 5: Add CPU↔wgpu parity tests**

Run one and 100 steps for Conway, Lenia, raw kernel, multi-channel,
multi-basis, frozen, Rate/Value and boundaries. Skip with an explicit test
message only when no non-CPU adapter is available.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --lib wgpu_backend
cargo test --locked --test backend_parity -- --include-ignored
```

Commit:

```sh
git add src/sim/wgpu_backend.rs src/sim/local_backend.rs src/sim/mod.rs tests/backend_parity.rs
git commit -m "feat: add portable wgpu simulation backend"
```

### Task 7: Adapt CUDA and implement visible backend selection/fallback

**Files:**
- Create: `src/sim/backend_selector.rs`
- Modify: `src/sim/cuda.rs`
- Modify: `src/sim/cuda_codegen.rs`
- Modify: `src/sim/service.rs`
- Modify: `src/sim/worker.rs`
- Create: `src/gui/widgets/backend_picker.rs`
- Modify: `src/gui/app.rs`
- Test: `src/sim/backend_selector.rs`
- Test: `tests/backend_parity.rs`

**Interfaces:**
- Produces: `BackendPolicy`, `BackendProbe`, `BackendSelector::candidates`.
- CUDA implements `LocalBackend` and owns device state between snapshots.

- [ ] **Step 1: Write deterministic probe-order tests**

```rust
#[test]
fn auto_orders_cuda_then_discrete_wgpu_then_integrated_wgpu_then_cpu() {
    let probes = fake_probes(cuda_ok(), amd_discrete(), intel_integrated());
    assert_eq!(
        BackendSelector::candidates(BackendPolicy::Auto, probes),
        vec![Candidate::Cuda, Candidate::Wgpu(AMD), Candidate::Wgpu(INTEL), Candidate::Cpu]
    );
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib backend_selector`

- [ ] **Step 3: Separate probe from construction**

CUDA reports compiled/driver/device/NVRTC reasons. wgpu reports adapter,
device type and violated limits. CPU is always available unless plan
allocation overflows.

- [ ] **Step 4: Stop per-step CUDA full-state round trips**

Upload at construction/edit; dispatch successive steps in device buffers;
read back only for snapshots/recovery. Preserve existing PTX/module caches.

- [ ] **Step 5: Implement fallback state machine**

On recoverable Auto failure, rebuild next candidate from the last confirmed
snapshot, publish a persistent notice and restore running state. Require
policies pause instead of changing kind.

- [ ] **Step 6: Add graphical Backend picker**

Show Auto order, actual device, every probe reason, current fallback notice
and explicit choices. Selection sends `SelectBackend`.

- [ ] **Step 7: Verify and commit**

Run:

```sh
cargo test --locked --lib backend_selector
cargo test --locked --test backend_parity
cargo test --locked --features cuda --lib sim::cuda
```

Commit:

```sh
git add src/sim src/gui/widgets/backend_picker.rs src/gui/app.rs tests/backend_parity.rs
git commit -m "feat: select and recover local compute backends"
```

---

## Phase C — Mouse-first GUI editors

### Task 8: Simulation canvas and shared coordinate transform

**Files:**
- Create: `src/gui/canvas/mod.rs`
- Create: `src/gui/canvas/transform.rs`
- Create: `src/gui/canvas/world.rs`
- Create: `src/gui/sections/simulation.rs`
- Modify: `src/gui/sections/mod.rs`
- Modify: `src/gui/app.rs`
- Refactor: `src/render/camera.rs`
- Refactor: `src/render/basis_scene.rs`
- Test: `src/gui/canvas/transform.rs`
- Test: `tests/gui_simulation.rs`

**Interfaces:**
- Produces: `CanvasTransform`, `WorldCanvasResponse`,
  `render_world_canvas(ui, snapshot, state)`.

- [ ] **Step 1: Write property and pointer-centered zoom tests**

```rust
#[test]
fn screen_world_round_trip_and_zoom_anchor_are_stable() {
    let mut t = CanvasTransform::new(rect(37.0, 19.0, 901.0, 613.0), [128.0, 128.0], 2.7);
    let pointer = pos2(411.25, 287.75);
    let before = t.screen_to_world(pointer);
    t.zoom_at(pointer, 1.25);
    assert_close(t.screen_to_world(pointer), before, 1e-9);
    assert_close(t.world_to_screen(before), pointer, 1e-4);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --lib gui::canvas::transform`

- [ ] **Step 3: Render actual raster and polygon scenes**

Use black domain interior, deep-blue exterior, channel composite and the same
transform for paint hit testing. Upload changed snapshot generations to an
egui texture; do not recreate textures for unchanged snapshots.

- [ ] **Step 4: Implement visible controls and gestures**

Buttons: Run/Pause, Step, Reset, Randomize, Clear, Fit, channel view, brush
value/radius. Gestures: left paint, right erase, middle pan, wheel zoom and
hover inspect.

- [ ] **Step 5: Add GUI interaction tests**

Click each visible control and assert worker command. Drag from two screen
points and assert WorldEdit coordinates derived from the same rendered
transform.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --lib gui::canvas
cargo test --locked --test gui_simulation
```

Commit:

```sh
git add src/gui/canvas src/gui/sections src/gui/app.rs src/render tests/gui_simulation.rs
git commit -m "feat: add interactive local simulation canvas"
```

### Task 9: Port the Tiling editor to an egui canvas

**Files:**
- Create: `src/gui/canvas/tiling.rs`
- Create: `src/gui/sections/tiling.rs`
- Modify: `src/gui/app.rs`
- Refactor: `src/workbench/tiling_editor.rs`
- Modify: `src/document/mod.rs`
- Test: `tests/gui_tiling.rs`

**Interfaces:**
- Produces: `TilingCanvasResponse`, graphical preset cards, construction
  controls and seam diagnostics.
- Consumes pure tiling scene/hit/solver commands.

- [ ] **Step 1: Write a complete mouse construction test**

```rust
#[test]
fn user_draws_undoes_and_closes_a_triangle_with_visible_neighbors() {
    let mut gui = tiling_gui_blank();
    click(&mut gui, "Draw from scratch");
    canvas_click(&mut gui, world(-0.5, -0.4));
    canvas_click(&mut gui, world(0.5, -0.4));
    canvas_click(&mut gui, world(0.0, 0.5));
    click(&mut gui, "Undo point");
    assert_eq!(gui.construction_vertices(), 2);
    click(&mut gui, "Redo point");
    click(&mut gui, "Finish polygon");
    assert_eq!(gui.draft_basis_count(), 1);
    assert!(gui.visible_neighbor_copies() >= 6);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test gui_tiling`

- [ ] **Step 3: Draw center cell and true adjacency ring**

Build neighbors from translation vectors/seams. Central editable polygons are
strong; adjacent periodic copies are translucent and noninteractive except
selection mapping.

- [ ] **Step 4: Reject invalid operations immediately**

Before appending a point reject duplicates, zero edges, intersections,
non-finite values and >64 vertices. Display reason next to pointer and keep the
draft unchanged.

- [ ] **Step 5: Add presets, solve and constrained drag**

Mouse cards for blank, square, triangles, hexagon and octagon+square. Solve
shows proposed full-edge pairs, residual and Accept/Cancel. Accepted seam
constraints make subsequent vertex drag move equivalence classes.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --test gui_tiling
cargo test --locked --lib sim::tiling
```

Commit:

```sh
git add src/gui/canvas/tiling.rs src/gui/sections/tiling.rs src/workbench/tiling_editor.rs src/document tests/gui_tiling.rs
git commit -m "feat: port periodic tiling design to the GUI"
```

### Task 10: Build truthful graphical Channels management

**Files:**
- Create: `src/gui/widgets/object_strip.rs`
- Create: `src/gui/canvas/channels.rs`
- Create: `src/gui/sections/channels.rs`
- Refactor: `src/workbench/channel_editor.rs`
- Modify: `src/document/mod.rs`
- Test: `tests/gui_channels.rs`

**Interfaces:**
- Produces reusable stable-ID object cards.
- Produces `ChannelPreviewSource::{Live,DraftInitial}` and
  `ChannelView::{Composite,Solo,Grid}`.

- [ ] **Step 1: Write the full three-channel lifecycle test**

```rust
#[test]
fn cards_support_add_select_rgb_hide_freeze_delete_and_undo() {
    let mut gui = one_channel_gui();
    click(&mut gui, "Add channel");
    click(&mut gui, "Add channel");
    assert_eq!(gui.channel_cards(), ["state", "channel_2", "channel_3"]);
    assert_eq!(gui.channel_colors(), [RED, GREEN, BLUE]);
    click_card(&mut gui, ChannelId(1));
    click_card_action(&mut gui, ChannelId(1), "Hide");
    click_card_action(&mut gui, ChannelId(2), "Freeze");
    click_card_action(&mut gui, ChannelId(1), "Delete");
    click(&mut gui, "Undo");
    assert_eq!(gui.selected_channel(), ChannelId(1));
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test gui_channels`

- [ ] **Step 3: Render all cards and actions**

Every card shows swatch, visible, active/frozen and delete. Add is trailing.
Card strip scrolls; no object is hidden behind a Next shortcut.

- [ ] **Step 4: Separate Live and Draft initial**

Live always uses active spec/snapshot. Draft initial always uses draft spec and
initial values. Structural mismatch defaults to fitted Draft, labels Live old
structure, and exposes Apply & Run. Never call draft initial as an implicit
Live fallback.

- [ ] **Step 5: Add view and color interactions**

Composite/Solo/Grid tabs, color popover with presets and exact RGB, hide/show,
freeze/unfreeze. Inspector contains channel-scope counts only.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --test gui_channels
cargo test --locked --lib channel
```

Commit:

```sh
git add src/gui/widgets/object_strip.rs src/gui/canvas/channels.rs src/gui/sections/channels.rs src/workbench/channel_editor.rs src/document tests/gui_channels.rs
git commit -m "feat: add truthful graphical channel management"
```

### Task 11: Build complete multi-Kernel management and editing

**Files:**
- Create: `src/gui/canvas/kernel.rs`
- Create: `src/gui/sections/kernels.rs`
- Create: `src/gui/widgets/numeric_popover.rs`
- Create: `src/gui/widgets/decision_dialog.rs`
- Refactor: `src/workbench/kernel_editor.rs`
- Refactor: `src/workbench/decision.rs`
- Modify: `src/document/mod.rs`
- Test: `tests/gui_kernels.rs`

**Interfaces:**
- Produces `KernelCardModel`, thumbnail cache, Weight/Support palette and
  exact value popover.
- Consumes selected Binding and stable KernelId.

- [ ] **Step 1: Write a nonsequential four-kernel journey**

```rust
#[test]
fn four_kernels_can_be_added_switched_edited_and_deleted_by_mouse() {
    let mut gui = one_kernel_gui();
    for _ in 0..3 { click(&mut gui, "Add kernel"); }
    assert_eq!(gui.kernel_card_count(), 4);
    for id in [KernelId(3), KernelId(0), KernelId(2), KernelId(1)] {
        click_kernel_card(&mut gui, id);
        assert_eq!(gui.selected_kernel(), id);
        paint_distinct_value(&mut gui, id.0 as f32 / 10.0);
    }
    delete_kernel_card(&mut gui, KernelId(1));
    assert_eq!(gui.kernel_card_count(), 3);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test gui_kernels`

- [ ] **Step 3: Add cards and cached thumbnails**

Each selected Binding shows all kernel cards, ordinal, symbol, source, actual
support thumbnail and delete. Add selects immediately. Click any card updates
canvas, Inspector and Growth.

- [ ] **Step 4: Render and hit-test actual polygon/raster cells**

Use one CanvasTransform for rendering/hits. Display positive, negative, active
zero, inactive, anchor, source basis and selection with persistent legend.
All cells remain reachable through pan/zoom/Fit.

- [ ] **Step 5: Implement mouse editing**

Weights/Support buttons; click/drag paint; right zero/deactivate; wheel active
value 0.05, Shift 0.005, Ctrl 0.5; empty wheel zoom; middle pan; double-click
exact value popover.

- [ ] **Step 6: Implement metadata controls**

Clickable source/output selectors, Affine/World metric, Gaussian, sigma,
stencil size/anchor and Reset RuleSet. No property is keyboard-only.

- [ ] **Step 7: Implement safe referenced deletion**

Dialog offers Cancel and Replace references with 0 and remove. Preview exact
source rewrite. Validate the compound draft before one transaction.

- [ ] **Step 8: Verify and commit**

Run:

```sh
cargo test --locked --test gui_kernels
cargo test --locked --lib kernel
```

Commit:

```sh
git add src/gui/canvas/kernel.rs src/gui/sections/kernels.rs src/gui/widgets src/workbench/kernel_editor.rs src/workbench/decision.rs src/document tests/gui_kernels.rs
git commit -m "feat: add mouse-first multi-kernel editor"
```

### Task 12: Build the Growth source editor and precise plot

**Files:**
- Create: `src/gui/canvas/growth.rs`
- Create: `src/gui/sections/growth.rs`
- Create: `src/gui/widgets/code_editor.rs`
- Refactor: `src/workbench/growth_editor.rs`
- Refactor: `src/workbench/growth_graph.rs`
- Modify: `src/sim/growth/typecheck.rs`
- Modify: `src/document/mod.rs`
- Test: `tests/gui_growth.rs`

**Interfaces:**
- Produces referenced external symbol analysis, `PlotAxes`, pinned inputs,
  curve/heatmap scene and syntax Help model.

- [ ] **Step 1: Write signature/arity and axis tests**

```rust
#[test]
fn adding_kernels_updates_signature_but_only_referenced_inputs_choose_axes() {
    let mut gui = growth_gui_with_four_kernels_source("gauss(k3, 0.5, 0.1)");
    assert_eq!(gui.signature_kernel_count(), 4);
    assert_eq!(gui.plot_axes(), PlotAxes::Curve(Symbol::Kernel(KernelId(3))));
    click_axis_chip(&mut gui, KernelId(1), Axis::Y);
    assert_eq!(gui.plot_axes(), PlotAxes::Heatmap(KernelId(3), KernelId(1)));
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test gui_growth`

- [ ] **Step 3: Add central source editor**

Use multiline TextEdit with custom lexer layouter, line gutter, visible cursor,
selection and inline diagnostic spans. Signature, binding and kernel chips are
above it. Clicking a kernel chip navigates to Kernels.

- [ ] **Step 4: Add referenced-symbol axis defaults**

Typed program exposes referenced external symbols. Zero kernel refs defaults
to self curve; one to that kernel; two or more to first two referenced in
signature order. User chip selections override by stable symbol.

- [ ] **Step 5: Draw curve/heatmap with egui Painter/wgpu**

Show axes, numerical ranges, Rate/Value label, zero reference, pinned values,
isolated equality markers, stale overlay and no-finite-sample message.

- [ ] **Step 6: Add complete mouse controls and Help**

Rate/Value toggle, min/max numeric fields, X/Y chips, pinned sliders/exact
fields, Properties/Help tabs and scrollable syntax/built-ins.

- [ ] **Step 7: Verify and commit**

Run:

```sh
cargo test --locked --test gui_growth
cargo test --locked --lib sim::growth
```

Commit:

```sh
git add src/gui/canvas/growth.rs src/gui/sections/growth.rs src/gui/widgets/code_editor.rs src/workbench/growth_editor.rs src/workbench/growth_graph.rs src/sim/growth/typecheck.rs src/document tests/gui_growth.rs
git commit -m "feat: add graphical growth programming workspace"
```

---

## Phase D — Persistence, legacy removal, validation and release

### Task 13: Complete Experiment review and local persistence

**Files:**
- Create: `src/gui/sections/experiment.rs`
- Create: `src/document/persistence.rs`
- Modify: `src/workbench/experiment_editor.rs`
- Modify: `src/gui/app.rs`
- Modify: `src/main.rs`
- Test: `tests/gui_experiment.rs`
- Test: `tests/persistence.rs`

**Interfaces:**
- Produces local Apply & Run, Open/Save/Save As, autosave, recovery and
  `GuiSettings`.

- [ ] **Step 1: Write atomic Apply and persistence tests**

```rust
#[test]
fn failed_candidate_keeps_active_world_and_successful_apply_runs() {
    let mut app = gui_fixture();
    let active = app.active_audit();
    app.set_invalid_growth("unknown()");
    click(&mut app, "Apply & Run");
    assert_eq!(app.active_audit(), active);
    app.set_growth("self");
    click(&mut app, "Apply & Run");
    assert!(app.snapshot().running);
    assert!(app.snapshot().revision > active.revision);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test gui_experiment --test persistence`

- [ ] **Step 3: Build graphical Experiment summary**

Cards show world, bases, all/active/frozen channels, Binding formula and
count, selected/all effective kernels, Growth, dt and backend probes.
Diagnostics navigate to exact sections.

- [ ] **Step 4: Implement atomic local files**

Keep old RON imports. Add settings.ron. Save via sibling temporary, flush,
sync, rename and Unix 0600. Autosave immutable snapshots off GUI thread.

- [ ] **Step 5: Implement Open/Save and recovery dialogs**

Use native path dialog only if its cross-platform dependency passes all
targets; otherwise provide recent/default workspace plus explicit path field.
No file operation blocks update.

- [ ] **Step 6: Verify and commit**

Run:

```sh
cargo test --locked --test gui_experiment --test persistence
cargo test --locked --lib document
```

Commit:

```sh
git add src/gui/sections/experiment.rs src/document/persistence.rs src/workbench/experiment_editor.rs src/gui/app.rs src/main.rs tests
git commit -m "feat: apply run and persist local GUI experiments"
```

### Task 14: Make GUI the only product and remove terminal/remote code

**Files:**
- Modify: `src/main.rs`
- Replace: `src/app.rs` with small GUI compatibility/export module or delete
- Delete: `src/tui/**`
- Delete: `src/render/display/**`
- Delete: `src/remote.rs`
- Delete/refactor: `src/input.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`, `Cargo.lock`
- Delete: terminal/remote-only tests and scripts
- Modify: `README.md`, `docs/releases.md`, `docs/performance.md`
- Test: `tests/workflow_contract.rs`

**Interfaces:**
- Final CLI contains local GUI flags only.
- Removes every server/connect/terminal interface.

- [ ] **Step 1: Write the final CLI contract test**

```rust
#[test]
fn remote_modes_are_removed_with_an_actionable_error() {
    assert!(parse_cli(["server"]).unwrap_err().contains("remote/server mode was removed"));
    assert!(parse_cli(["connect", "tinker"]).unwrap_err().contains("runs simulation locally"));
    assert_eq!(parse_cli(["--backend", "cpu"]).unwrap().backend, BackendPolicy::RequireCpu);
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --test workflow_contract`

- [ ] **Step 3: Switch default startup to GUI**

Remove temporary `--gui`. Keep `--experiment`, `--backend`,
`--safe-mode`, `--version`. Server/connect/ssh flags are explicit errors.

- [ ] **Step 4: Delete legacy only after a parity checklist review**

Search:

```sh
rg -n "ratatui|crossterm|Kitty|Sixel|half.block|run_server|run_connect|RemoteMessage|ssh.command" src Cargo.toml tests scripts
```

Each remaining result must be a migration error string or historical document,
not executable product code.

- [ ] **Step 5: Remove dependencies and obsolete tests**

Delete terminal display, shared-memory graphics, protocol, PTY and remote
journeys. Preserve model/backend/GUI agentic tests. Rewrite current README and
release docs as local GUI facts.

- [ ] **Step 6: Run complete local-product gates**

Run on tinker:

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --no-default-features --all-targets
cargo clippy --locked --all-targets -- -D warnings
git diff --check
```

- [ ] **Step 7: Commit**

```sh
git add -A
git commit -m "refactor: remove terminal and remote product modes"
```

### Task 15: Rebuild CI and package cross-platform GUI artifacts

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Create: `scripts/smoke-gui.sh`
- Create: `scripts/install-gui-local.sh`
- Modify: `docs/releases.md`
- Test: `tests/workflow_contract.rs`

**Interfaces:**
- Produces Linux/Windows/macOS x86_64/ARM64 GUI packages and SHA256SUMS.

- [ ] **Step 1: Write workflow contract assertions**

Assert every target, CPU-only gate, GUI smoke, checksum and stable release
step exists. Assert no packaged command advertises server/connect.

- [ ] **Step 2: Add Linux GUI dependencies and Xvfb smoke**

Install X11/Wayland development libraries and Mesa/Vulkan runtime. Start the
Release GUI under Xvfb with a clean data directory, capture one screenshot,
assert window remains alive and closes cleanly.

- [ ] **Step 3: Add cross-platform build/package smoke**

Build all six current targets. Launch where runner architecture permits.
Package the executable and required desktop metadata/app bundle files.

- [ ] **Step 4: Add hardware parity jobs**

CUDA runner executes CUDA/CPU parity. Intel GPU runner executes wgpu/CPU parity
before stable release. A non-Intel integrated target is recorded in the
release evidence.

- [ ] **Step 5: Verify workflow locally without building on Pi**

Run workflow contract tests and YAML syntax checks on tinker.

- [ ] **Step 6: Commit**

```sh
git add .github scripts docs/releases.md tests/workflow_contract.rs
git commit -m "ci: package and validate the local GUI application"
```

### Task 16: Execute the complete real-user agentic GUI test

**Files:**
- Create: `docs/testing/agentic-gui.md`
- Create: `tests/agentic/gui-full-journey.md`
- Replace: `scripts/agentic/*.sh` with GUI lifecycle/capture scripts
- Create evidence: `target/agentic-gui/<candidate-sha>/`
- Modify code/tests for every discovered defect before final evidence

**Interfaces:**
- Consumes the exact checksum-verified ARM64 candidate.
- Produces manifest, environment, action log, before/after PNGs, observations
  and final PASS/FAIL.

- [ ] **Step 1: Download, verify and install the candidate on Raspberry Pi**

Do not invoke Cargo. Record archive URL, SHA256, binary `--version`, commit,
OS, renderer, compute probes and screen geometry.

- [ ] **Step 2: Start a clean real GUI session**

Use isolated XDG data/config directories. Start lightweight X11/Wayland
session, one Cellarium process, and verify no orphan process from prior runs.

- [ ] **Step 3: Perform the mandatory journey from the spec**

The Agent must actually click, drag, scroll, type, inspect and adapt:

- Simulation controls and canvas;
- blank triangle draw/undo/close;
- hex neighbor geometry;
- multi-polygon seam solve and constrained edit;
- three Channels with RGB/view/color/hide/freeze/delete/undo;
- four Kernels selected nonsequentially and edited distinctly;
- referenced Kernel deletion cancel and resolution;
- Growth arity/source/invalid/curve/heatmap/axes/pins/Rate/Value;
- Apply & Run;
- Auto/CPU backend changes;
- save, close and reopen;
- resize and repeated coordinate interaction stress.

- [ ] **Step 4: Judge usability, not just mutations**

After every task answer: could a new user discover the next action, did the
visual result match intent, did any stale/blank/overlapping frame appear, and
was failure recovery understandable? Any “works but unclear” row is FAIL.

- [ ] **Step 5: Fix and repeat until all rows pass**

For each defect: reproduce with a focused deterministic test, implement,
remote-build a new candidate, reinstall, and repeat the affected journey plus
regression neighbors. Never edit the evidence of an older candidate to claim a
new candidate passed.

- [ ] **Step 6: Run final process/resource audit**

After exit verify no Cellarium, Xvfb/session owned by the test, child process,
temporary candidate or locked workspace remains. Preserve only evidence.

- [ ] **Step 7: Commit the testing contract and code fixes**

```sh
git add docs/testing tests/agentic scripts/agentic src
git commit -m "test: certify complete local GUI user journeys"
```

### Task 17: Final verification and stable release

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/releases.md`
- Modify: `README.md`
- Tag/release after all gates

**Interfaces:**
- Produces one stable version and matching cross-platform assets.

- [ ] **Step 1: Run fresh remote verification**

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --no-default-features --all-targets
cargo clippy --locked --all-targets -- -D warnings
git diff --check
```

- [ ] **Step 2: Review exact candidate evidence**

Confirm candidate SHA equals final commit, all GUI agentic rows PASS, Intel
wgpu and CUDA/CPU parity evidence is attached, and no old server/connect
artifact appears.

- [ ] **Step 3: Update stable version and docs**

Document local-only architecture, backend fallback and platform runtime
requirements. Remove the README migration notice because the migration is now
the released product.

- [ ] **Step 4: Push branch, merge through the repository's normal review and tag**

Tag exactly `v<Cargo.toml version>`. Do not use a prerelease flag.

- [ ] **Step 5: Verify published assets**

Download every asset, verify SHA256SUMS, start native smoke where possible and
re-run the Raspberry Pi launch/save/reopen smoke using the published ARM64
asset.

- [ ] **Step 6: Publish the final stable release report**

Report version, commit, assets, backend evidence, agentic evidence path and
known non-blocking limitations. Do not state completion if any required gate
was skipped.

---

## Plan Self-Review Checklist

- [ ] Every spec section maps to at least one task.
- [ ] No task assumes server/connect/SSH remains in the product.
- [ ] Intel integrated wgpu is an explicit gate, not inferred from compilation.
- [ ] CPU-only build still includes the GUI.
- [ ] CUDA, wgpu and CPU consume one ComputePlan.
- [ ] Simulation work never blocks the egui update thread.
- [ ] Every primary editor operation has a mouse route.
- [ ] Multi-Kernel validation switches and edits every created Kernel.
- [ ] Growth arity and axes derive from the selected Binding.
- [ ] Persistence imports old data without destructive normalization.
- [ ] Legacy deletion happens only after GUI parity.
- [ ] Final agentic test uses exact packaged Release and real visual judgment.
