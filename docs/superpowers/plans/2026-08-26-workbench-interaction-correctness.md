# Workbench Interaction Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every Simulation and Workbench view visually truthful, directly operable, and verifiable through the released ARM64 client.

**Architecture:** Add explicit raster-scene presence to the existing Workbench presenter, then fix state ownership at each producer: construction, topology preview, camera, kernel editor, growth sampler, and responsive TUI controls. Keep the current simulation and Kitty display architecture; prove each behavior with focused Rust tests, protocol/PTY journeys on tinker, and a final real X11/Kitty journey using the stable GitHub Release artifact.

**Tech Stack:** Rust, ratatui/crossterm, Kitty graphics protocol, existing CPU/CUDA backends, GitHub Actions, Xvfb/Openbox/Kitty/xdotool.

**Spec:** `docs/superpowers/specs/2026-08-26-workbench-interaction-correctness-design.md`

## Global Constraints

- Do not build or run performance workloads on the local Raspberry Pi.
- Compile, unit-test, integration-test, and package on tinker or CI.
- Local agentic validation must download the ARM64 artifact from the stable GitHub Release.
- Preserve direct local GPU rendering and the existing client/server mode.
- Preserve Kitty graphics as the default when terminal capability detection selects it.
- Preserve half-block fallback and verify its interaction paths separately.
- Do not normalize kernel potentials; growth functions continue receiving raw weighted sums.
- Do not rewrite the entire display presenter or simulation engine.
- Do not use subagents for this work.
- Publish a stable release, not a prerelease, only after all gates pass.
- Every interaction assertion observes the resulting model and rendered framebuffer; successful event injection alone is not a pass.

## File structure

- `src/render/workbench_graphics.rs`: scene identity, explicit presence transitions, placement lifecycle.
- `src/tui/workbench.rs`: Workbench layout, scene dispatch, responsive toolbar, row rectangles, empty-state widgets.
- `src/app.rs`: event routing, camera-fit policy, active/draft compatibility, selection notices.
- `src/workbench/state.rs`: construction invariants and dependency-safe draft mutations.
- `src/workbench/kernel_editor.rs`: one periodic pixel transform for render/hit and selection/edit permissions.
- `src/workbench/camera_fit.rs` (new): pure world-bounds-to-camera fit calculation.
- `src/sim/growth/plot.rs`: discontinuity-aware sample generation with real input coordinates.
- `src/workbench/growth_editor.rs`: retain `CurveData`, plot diagnostics, editor refresh.
- `src/workbench/growth_graph.rs`: input-coordinate plotting and isolated point markers.
- `tests/support/terminal_probe.rs`: semantic PTY journeys and frame/trace checkpoints.
- `tests/remote_e2e.rs`: protocol and half-block/Kitty E2E gates.
- `scripts/agentic-workbench-journey.sh` (new): reproducible real X11/Kitty journey without building locally.
- `docs/testing/agentic-workbench.md` (new): journey, prerequisites, observations, cleanup, and evidence schema.

---

### Task 1: Explicit graphics presence and stale-placement removal

**Files:**
- Modify: `src/render/workbench_graphics.rs`
- Modify: `src/app.rs:381-434`
- Modify: `src/tui/workbench.rs:400-760`
- Test: unit tests in the same three modules

**Interfaces:**
- Produces: `pub enum ScenePresence { Pixels, Empty, Text }`.
- Produces: `WorkbenchGraphicsSurface::transition(presence: ScenePresence, key: Option<SceneKey>) -> PlacementAction`.
- Consumes: the existing `SceneKey`, `PlacementAction`, generation counters, and display widgets.

- [ ] **Step 1: Write failing scene-transition tests**

Add tests that start with a presented Pixels key, transition to Empty or Text, and assert a delete/clear action even when section and placement generation did not otherwise change. Also queue an older Pixels generation, transition to Empty, complete the older generation, and assert it cannot be presented.

```rust
#[test]
fn pixels_to_empty_deletes_the_existing_placement() {
    let mut surface = WorkbenchGraphicsSurface::default();
    assert_eq!(
        surface.transition(ScenePresence::Pixels, Some(scene(1))),
        PlacementAction::Present
    );
    assert_eq!(
        surface.transition(ScenePresence::Empty, None),
        PlacementAction::Delete
    );
    assert_eq!(surface.current_presence(), ScenePresence::Empty);
}

#[test]
fn obsolete_pixels_cannot_reappear_after_empty() {
    let mut surface = WorkbenchGraphicsSurface::default();
    let token = surface.begin_frame(scene(7));
    surface.transition(ScenePresence::Empty, None);
    assert!(!surface.finish_frame(token));
}
```

- [ ] **Step 2: Run the focused tests and record RED**

Run on tinker:

```bash
cargo test --locked workbench_graphics -- --nocapture
```

Expected: FAIL because transitions do not represent Empty/Text and stale completion remains admissible.

- [ ] **Step 3: Implement explicit presence**

Add `ScenePresence` to the surface state. Require `Some(SceneKey)` only for Pixels, increment the epoch on Pixels→Empty/Text, and return `PlacementAction::Delete` when a real placement may exist. Keep Pixels→Pixels reuse semantics unchanged.

At the TUI call site derive presence before dispatch:

```rust
fn workbench_scene_presence(state: &WorkbenchState) -> ScenePresence {
    match state.section() {
        WorkbenchSection::Experiment => ScenePresence::Text,
        WorkbenchSection::Tiling
            if state.draft().tiling.is_none()
                && state.tiling_construction().is_empty() =>
        {
            ScenePresence::Empty
        }
        WorkbenchSection::Kernels if state.selected_kernel_definition().is_none() => {
            ScenePresence::Empty
        }
        _ => ScenePresence::Pixels,
    }
}
```

For Empty render an ordinary bordered message after scheduling delete; for Text render the existing widgets. Ensure half-block fills the previous canvas cell rectangle with spaces.

- [ ] **Step 4: Run focused rendering tests and verify GREEN**

```bash
cargo test --locked workbench_graphics -- --nocapture
cargo test --locked tui::workbench::tests::blank_tiling -- --nocapture
cargo test --locked tui::workbench::tests::empty_kernel -- --nocapture
```

Expected: PASS; a blank Tiling or missing kernel produces no retained pixels and has actionable text.

- [ ] **Step 5: Commit**

```bash
git add src/render/workbench_graphics.rs src/app.rs src/tui/workbench.rs
git commit -m "fix: clear obsolete workbench graphics"
```

### Task 2: Truthful polygon construction undo

**Files:**
- Modify: `src/workbench/state.rs:560-730`
- Modify: `src/app.rs:1080-1150`
- Test: `src/workbench/state.rs`
- Test: `src/app.rs`
- Test: `src/workbench/tiling_editor.rs`

**Interfaces:**
- Produces: `WorkbenchState::pop_tiling_vertex() -> Option<Vec2>` with pointer reset.
- Produces: `WorkbenchState::clear_tiling_interaction()`.
- Consumes: `tiling_construction`, `tiling_pointer`, `TilingScene::render_rgba`.

- [ ] **Step 1: Write a failing model-and-pixel test**

Construct three vertices with the pointer at vertex three, render, invoke Ctrl+Z through `handle_workbench_editor_key`, render again, and assert the model has two vertices, the pointer equals vertex two or is None, and pixels around removed vertex three are background.

```rust
assert_eq!(app.workbench().tiling_construction().len(), 2);
assert_ne!(app.workbench().tiling_pointer(), Some(removed));
assert!(removed_probe_pixels.iter().all(|pixel| *pixel == background));
```

Also test blank, cancel, close, section change, and tool change clear obsolete pointers.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked tiling_construction_undo_removes_pointer_preview -- --nocapture
```

Expected: FAIL because `tiling_pointer` retains the removed point.

- [ ] **Step 3: Implement the interaction invariant**

In `pop_tiling_vertex`, set the pointer to `construction.last().copied()` or None. Route blank/cancel/finish/tool/section transitions through `clear_tiling_interaction`; the method clears construction drag and pointer state in one place.

- [ ] **Step 4: Verify focused tests**

```bash
cargo test --locked tiling_pointer -- --nocapture
cargo test --locked tiling_construction -- --nocapture
```

Expected: PASS and no old triangle after undo.

- [ ] **Step 5: Commit**

```bash
git add src/workbench/state.rs src/app.rs src/workbench/tiling_editor.rs
git commit -m "fix: keep polygon construction preview truthful"
```

### Task 3: Separate authoritative runtime from incompatible drafts

**Files:**
- Modify: `src/app.rs:1740-1815`
- Modify: `src/tui/workbench.rs:580-630,810-850`
- Modify: `src/sim/tiling/mod.rs` or the existing topology equality helper module
- Test: `src/app.rs`
- Test: `src/tui/workbench.rs`

**Interfaces:**
- Produces: `App::workbench_runtime_matches_draft() -> bool` with complete structural equality.
- Produces: `enum ChannelPreviewSource { AuthoritativeRuntime, DraftInitial }`.
- Consumes: dimensions, ordered basis/channel IDs, translations, prototypes, instances, transforms.

- [ ] **Step 1: Add failing compatibility tests**

Cover equal cardinality but different square/hex topology, reordered basis IDs, changed transforms, changed dimensions, and an exactly identical clone.

```rust
app.workbench_mut().replace_tiling(hex_tiling());
assert_eq!(
    app.channel_preview_source(),
    ChannelPreviewSource::DraftInitial
);
assert!(!app.workbench_runtime_matches_draft());
```

Render the Channels header and assert it contains `Preview: draft initial state`, while the sampled values come from the draft initial field.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked workbench_runtime_matches_draft -- --nocapture
cargo test --locked channels_incompatible_draft_uses_initial_preview -- --nocapture
```

Expected: square→hex with one basis incorrectly reports compatible.

- [ ] **Step 3: Implement structural comparison and explicit source**

Compare the complete ordered model, not only counts. Centralize the source decision:

```rust
pub fn channel_preview_source(&self) -> ChannelPreviewSource {
    if self.workbench_runtime_matches_draft() {
        ChannelPreviewSource::AuthoritativeRuntime
    } else {
        ChannelPreviewSource::DraftInitial
    }
}
```

Use the enum for both the canvas values and label, so pixels and description cannot disagree.

- [ ] **Step 4: Verify focused tests**

Run the two commands from Step 2 and the existing basis-scene suite. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/tui/workbench.rs src/sim/tiling
git commit -m "fix: separate draft and runtime channel previews"
```

### Task 4: Geometry-aware one-time camera fit

**Files:**
- Create: `src/workbench/camera_fit.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/render/basis_scene.rs`
- Test: `src/workbench/camera_fit.rs`
- Test: `src/app.rs`

**Interfaces:**
- Produces: `pub struct WorldBounds { pub min: [f64; 2], pub max: [f64; 2] }`.
- Produces: `pub fn fit_camera(bounds: WorldBounds, pixel_size: [u32; 2], margin: f64) -> Option<SceneCamera>`.
- Produces: `App::fit_simulation_camera_if_needed(pixel_size: [u32; 2])`.
- Consumes: raster dimensions or basis-domain transformed polygon bounds.

- [ ] **Step 1: Write failing pure fit tests**

Test 256×256 in a 1360×900 viewport, an oblique hex domain, degenerate bounds, and aspect-ratio preservation.

```rust
let camera = fit_camera(
    WorldBounds { min: [0.0, 0.0], max: [256.0, 256.0] },
    [1360, 900],
    0.05,
).unwrap();
assert!(camera.zoom > 3.0);
assert_eq!(camera.center, [128.0, 128.0]);
```

At App level verify first viewport fits, manual wheel zoom disables future automatic fits, `0` explicitly refits, and geometry-changing Apply re-arms one fit.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked camera_fit -- --nocapture
cargo test --locked initial_viewport_auto_fits -- --nocapture
```

Expected: FAIL because initial camera remains zoom 1.0.

- [ ] **Step 3: Implement fit policy**

Compute finite bounds from every transformed prototype vertex across the finite basis domain. For raster topology use `[0,width] × [0,height]`. Fit the limiting axis with a five-percent margin. Track `camera_user_modified` and `camera_fit_pending`; set pending on initial viewport, geometry-changing Apply, and persisted-load first view.

- [ ] **Step 4: Verify rendering**

```bash
cargo test --locked camera_fit -- --nocapture
cargo test --locked basis_scene -- --nocapture
cargo test --locked initial_viewport_auto_fits -- --nocapture
```

Expected: PASS, including oblique hex bounds.

- [ ] **Step 5: Commit**

```bash
git add src/workbench/camera_fit.rs src/workbench/mod.rs src/app.rs src/render/basis_scene.rs
git commit -m "fix: fit simulation camera to domain geometry"
```

### Task 5: Select every visible kernel cell and explain empty/zero states

**Files:**
- Modify: `src/workbench/kernel_editor.rs:387-900`
- Modify: `src/app.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/kernel_editor.rs`
- Test: `src/app.rs`

**Interfaces:**
- Produces: `PeriodicKernelScene::selection_in_pixel_rect(rect) -> Option<PeriodicKernelSelection>` without tool filtering.
- Produces: `PeriodicKernelScene::edit_permission(selection, tool) -> KernelEditPermission`.
- Consumes: the same `PeriodicPixelTransform` for polygon rendering and hit-testing.

- [ ] **Step 1: Write failing reachability and permission tests**

For every rendered polygon center and for quantized points along the visible perimeter, assert selection round-trips to the same offset/basis after fit, pan, and zoom. Assert inactive cells select in Weights mode but return `RequiresSupportMode`.

```rust
let selected = scene.selection_in_pixel_rect(cell.pixel_rect).unwrap();
assert_eq!(selected, cell.selection);
assert_eq!(
    scene.edit_permission(selected, KernelTool::Weights),
    KernelEditPermission::RequiresSupportMode
);
```

Add snapshots for no kernel and all-zero kernel: no kernel must show `A Add kernel`; zero kernel must still contain outlines, anchor, and a zero legend.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked periodic_kernel_selection -- --nocapture
cargo test --locked kernel_empty_state -- --nocapture
```

Expected: inactive cells are filtered and empty/zero canvases are visually ambiguous.

- [ ] **Step 3: Split inspection from mutation**

Remove `tool_accepts` from hit-testing. Always update selection first. Gate the following mutation by `KernelEditPermission`; on inactive Weights edit keep selection and set `Switch to Support mode to activate this cell`. Render inactive and zero cells with distinguishable outlines and an anchor marker.

- [ ] **Step 4: Verify focused tests**

Run the two commands from Step 2 plus `cargo test --locked kernel_editor -- --nocapture`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workbench/kernel_editor.rs src/app.rs src/tui/workbench.rs
git commit -m "fix: make periodic kernel cells inspectable"
```

### Task 6: Reject kernel deletion that would invalidate Growth

**Files:**
- Modify: `src/workbench/state.rs:1850-1915`
- Modify: `src/app.rs:610-650`
- Modify: `src/workbench/growth_editor.rs`
- Test: `src/workbench/state.rs`
- Test: `src/app.rs`

**Interfaces:**
- Produces: `WorkbenchState::remove_last_kernel_for_selected() -> Result<(), String>` with preflight typecheck.
- Consumes: proposed post-removal kernel-input signature and existing growth source.
- Guarantees: successful history entries are type-valid; rejection does not dirty state.

- [ ] **Step 1: Write failing atomicity tests**

Create two kernels and Growth source that references the second generated symbol. Attempt deletion and assert the exact symbol is named, kernel count and history are unchanged, and no dirty transition occurs. Then remove the reference, delete, and assert undo restores kernel and signature together.

```rust
let before = state.clone();
let error = state.remove_last_kernel_for_selected().unwrap_err();
assert!(error.contains("k2"));
assert_eq!(state.draft(), before.draft());
assert_eq!(state.history_len(), before.history_len());
```

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked kernel_removal_preserves_growth_signature -- --nocapture
```

Expected: deletion succeeds and later validation reports `unknown_symbol`.

- [ ] **Step 3: Add preflight validation**

Build the proposed input list, compile/typecheck the unchanged source with it, and reject before mutation if diagnostics contain the removed symbol. On success mutate kernels and growth inputs as one history transaction, then refresh editor signature and plot.

- [ ] **Step 4: Verify focused tests**

```bash
cargo test --locked kernel_removal -- --nocapture
cargo test --locked growth_signature -- --nocapture
```

Expected: PASS; no delayed `unknown_symbol`.

- [ ] **Step 5: Commit**

```bash
git add src/workbench/state.rs src/app.rs src/workbench/growth_editor.rs
git commit -m "fix: preserve growth validity when removing kernels"
```

### Task 7: Render discontinuous Growth programs faithfully

**Files:**
- Modify: `src/sim/growth/plot.rs`
- Modify: `src/workbench/growth_editor.rs`
- Modify: `src/workbench/growth_graph.rs`
- Test: those three modules

**Interfaces:**
- Produces: `CurveData { axis: String, samples: Vec<CurveSample>, diagnostics: Vec<PlotDiagnostic> }`.
- Produces: sorted unique `CurveSample { input: f32, value: Option<f32>, trace: Option<_>, kind: CurveSampleKind }`.
- Produces: `CurveSampleKind::{Uniform, BelowThreshold, ExactThreshold, AboveThreshold}`.
- Consumes: typed Growth AST and constant-foldable comparison operands.

- [ ] **Step 1: Write failing equality-plot tests**

Compile `if potential == 2/6 || potential == 3/6 { 1 } else { 0 }`, sample over `[0,1]`, and assert exact samples exist at both rational thresholds with value 1 while immediate neighbors are 0. Render and assert colored point pixels occur away from the zero baseline.

```rust
assert_eq!(curve.value_at_exact(2.0 / 6.0), Some(1.0));
assert_eq!(curve.value_at_exact(3.0 / 6.0), Some(1.0));
assert!(frame.has_isolated_marker_above_baseline());
```

Also test reversed comparisons, constants outside the domain, invalid values, and ordinary smooth curves.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked equality_thresholds_are_visible -- --nocapture
cargo test --locked growth_graph -- --nocapture
```

Expected: no uniform sample lands exactly on both equality thresholds and the graph is flat.

- [ ] **Step 3: Implement bounded critical probes**

Walk typed comparisons whose one side is the chosen axis and other side constant-folds. Add `next_down(threshold)`, exact threshold, and `next_up(threshold)` when in-domain, bounded by the existing 4096-sample budget. Sort/deduplicate by input. Store real x inputs in `GrowthScene`; map x from the input interval. Connect only compatible continuous samples and draw a 3–5 pixel marker for exact isolated values. Add a diagnostic such as `2 isolated thresholds`.

- [ ] **Step 4: Verify focused tests**

```bash
cargo test --locked sim::growth::plot -- --nocapture
cargo test --locked growth_graph -- --nocapture
cargo test --locked growth_editor -- --nocapture
```

Expected: PASS without changing runtime Growth evaluation or potential normalization.

- [ ] **Step 5: Commit**

```bash
git add src/sim/growth/plot.rs src/workbench/growth_editor.rs src/workbench/growth_graph.rs
git commit -m "fix: show discontinuities in growth plots"
```

### Task 8: Responsive toolbar, clickable Channels, and release-level journeys

**Files:**
- Modify: `src/tui/workbench.rs:100-220`
- Modify: `src/app.rs:436-520`
- Modify: `tests/support/terminal_probe.rs`
- Modify: `tests/remote_e2e.rs`
- Create: `scripts/agentic-workbench-journey.sh`
- Create: `docs/testing/agentic-workbench.md`
- Modify: `.github/workflows/release.yml` only if the stable artifact naming or smoke gate is missing

**Interfaces:**
- Produces: `ToolbarLayout { rows: Vec<ToolbarRow>, height: u16 }` where rendered segments and hit rectangles are identical.
- Produces: `channel_row_rects(state, inspector: Rect) -> Vec<(ChannelId, Rect)>`.
- Produces: JSON evidence with artifact SHA, observed scene hashes, semantic checkpoints, and cleanup status.

- [ ] **Step 1: Write failing responsive/control tests**

At minimum-supported and wide widths, build `ToolbarLayout`, render it, click the center of every visible action rectangle, and assert the expected command. Render three Channels rows, click each rectangle, and assert selected ID changes. Assert clicking whitespace changes only focus.

```rust
for item in toolbar_layout(&state, width).items() {
    assert_eq!(toolbar_action_at(&layout, item.rect.center()), Some(item.action));
}
for (channel, rect) in channel_row_rects(&state, inspector) {
    click(&mut app, rect.center());
    assert_eq!(app.workbench().selected_channel(), channel);
}
```

Extend the terminal probe with semantic checkpoints for all eleven regressions. Each action waits for its own later generation/trace and validates framebuffer content; do not accept fixed sleep or a prior frame.

- [ ] **Step 2: Run and record RED**

```bash
cargo test --locked toolbar_layout -- --nocapture
cargo test --locked channel_row_click -- --nocapture
cargo test --locked --test remote_e2e -- --nocapture
```

Expected: one-line toolbar clips actions, Inspector click does not select rows, and new journey checkpoints fail until Tasks 1–7 are present.

- [ ] **Step 3: Implement shared responsive rectangles**

Tokenize actions into stable labeled segments; greedily wrap at the actual inner width up to a bounded header height. Pass the same `ToolbarLayout` to rendering and hit-testing. Derive Channels row rectangles from the same lines used for rendering. Add a Growth help `top–bottom / total` scroll indicator.

- [ ] **Step 4: Complete remote verification on tinker**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
cargo test --locked --test remote_e2e -- --ignored --nocapture
git diff --check
```

Expected: every command exits 0. Capture protocol version, server revision/ack, Kitty consumed-frame cadence, and half-block semantic checkpoints. Kill only test-owned server processes and verify no owned Cellarium process remains.

- [ ] **Step 5: Commit implementation and journey**

```bash
git add src/tui/workbench.rs src/app.rs tests/support/terminal_probe.rs tests/remote_e2e.rs scripts/agentic-workbench-journey.sh docs/testing/agentic-workbench.md .github/workflows/release.yml
git commit -m "test: gate workbench with real interaction journeys"
```

- [ ] **Step 6: Publish one stable release**

Push the reviewed branch, merge as required by the repository workflow, create a stable version tag, and wait for the release workflow. Do not mark the release as prerelease. Record the Git commit and every asset SHA-256.

- [ ] **Step 7: Download and run the real ARM64 release journey locally**

Do not run Cargo locally. Download the ARM64 Release asset into a fresh `mktemp -d`, verify its SHA-256, and run `scripts/agentic-workbench-journey.sh` with an isolated `XDG_DATA_HOME`. The script starts test-owned Xvfb/Openbox/Kitty, drives real mouse/keyboard input, and stores screenshots after each semantic checkpoint.

The visual journey must:

1. verify initial domain auto-fit;
2. blank Tiling and observe old placement disappear;
3. draw a triangle, undo vertex three, and observe two-vertex geometry;
4. close/select hex, inspect neighboring cells, Apply & Run;
5. change unapplied topology and observe Channels draft-initial label;
6. create RGB channels and click each Inspector row;
7. inspect inactive kernel cells, change support, wheel and exact values;
8. reject unsafe kernel deletion immediately;
9. enter the equality Growth program and observe isolated markers;
10. exercise narrow/wide toolbars and every section transition;
11. repeat critical interaction paths in half-block.

Any failure creates a new focused RED test on tinker, followed by another implementation/release/download/agentic cycle.

- [ ] **Step 8: Verify cleanup and final evidence**

Assert there are no test-owned Cellarium, Kitty, Openbox, or Xvfb processes. Final evidence must include stable release URL, tag, commit, ARM64 SHA-256, tinker test commands, per-step screenshot paths, and a defect table showing all eleven original issues plus any newly discovered issues as PASS.

