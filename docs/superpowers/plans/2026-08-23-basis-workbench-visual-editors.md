# Basis Workbench Visual Editors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the prototype Workbench with discoverable, high-resolution Tiling, Kernel, Growth, and Channel editors that share exact rendering/input transforms in Kitty and half-block modes.

**Architecture:** One immutable `SceneTransform` is produced by layout and consumed by both RGBA rasterization and hit-testing. Typed `WorkbenchAction`s mutate the semantic draft and generate Undo inverses. Section-specific scene/controller modules feed one graphics surface; TUI modules render navigation, source editor, inspector, help, and width-safe footer around that surface.

**Tech Stack:** Rust 2024, crossterm, ratatui, existing CPU RGBA graphics surface, Kitty Graphics Protocol, half-block conversion.

**Spec:** `docs/superpowers/specs/2026-08-23-basis-aware-workbench-agentic-validation-design.md`

## Global Constraints

- Run all Rust builds/tests on tinker; never build on the Raspberry Pi.
- Render and hit-test through the same transform generation.
- Core source/kernel/tiling editing belongs in the central canvas, not only in the Inspector.
- Kitty and half-block use the same actions and logical scenes.
- Section/mode/resize/fallback/exit transitions delete the currently presented Kitty placement before presenting a replacement.
- Default one-channel display is near-white on black; exactly three channels default to RGB; exterior remains dark navy.

---

### Task 1: Shared scene transform and presentation lifecycle

**Files:**
- Create: `src/render/scene_transform.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/render/workbench_graphics.rs`
- Modify: `src/render/display/mod.rs`
- Modify: `src/app.rs`

**Interfaces:**
- Produces: `SceneCamera { center: [f64; 2], pixels_per_unit: f64 }` and `SceneTransform { generation, terminal_rect, pixel_size, camera }`.
- Produces: `terminal_to_pixel`, `pixel_to_world`, and `world_to_pixel`, each returning `None` outside the current placement.
- Produces: `GraphicsSurface::transition(SceneKey) -> PlacementAction`, where changing key requires deletion before new presentation.

- [ ] **Step 1: Write failing round-trip and stale-generation tests**

```rust
#[test]
fn terminal_pixel_world_round_trip_survives_resize_and_pan() {
    let t = SceneTransform::new(Rect::new(3, 2, 80, 40), [1280, 640], SceneCamera::new([2.0, -1.0], 3.5), 7);
    let terminal = [41, 19];
    let world = t.pixel_to_world(t.terminal_to_pixel(terminal).unwrap());
    assert_eq!(t.world_to_terminal(world), Some(terminal));
}
```

Assert events carrying generation 6 are rejected after resize produces generation 7.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib render::scene_transform render::workbench_graphics`

- [ ] **Step 3: Implement the immutable transform and SceneKey lifecycle**

`SceneKey` includes Workbench section, selected basis/channel/kernel, display mode, transform generation, and draft scene generation. Store no independent coordinate math in editor controllers.

- [ ] **Step 4: Add placement transition tests**

Use the existing TestBackend to assert the byte order is `delete old image` then `present new image` for section switch, Workbench exit, resize, Kitty failure fallback, disconnect, and normal exit. Assert repeated draws of one generation produce no new image ID and no fresh-frame increment.

- [ ] **Step 5: Run GREEN and commit**

Run: `cargo test --lib render::scene_transform render::workbench_graphics render::display`

```bash
git add src/render src/app.rs
git commit -m "fix: unify workbench rendering and hit testing"
```

### Task 2: Typed interaction router, shell, focus, help, and footer

**Files:**
- Create: `src/workbench/action.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/input.rs`
- Replace: `src/tui/workbench.rs` with `src/tui/workbench/mod.rs`
- Create: `src/tui/workbench/layout.rs`
- Create: `src/tui/workbench/footer.rs`
- Create: `src/tui/workbench/help.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/app.rs`

**Interfaces:**
- Produces: `WorkbenchAction` variants for navigation, selection, tools, pointer Down/Move/Drag/Up, wheel, text edit, numeric edit, Undo/Redo, Apply, and Revert.
- Produces: `RemoteCorrelation::{InputSequence(u64), ApplyRequest(u64)}`.
- Produces: `WorkbenchController::handle(action, transform_generation) -> InteractionReceipt`.
- Produces: `InteractionReceipt { ui_sequence: u64, draft_generation: u64, scene_generation: u64, remote: Option<RemoteCorrelation> }`.

- [ ] **Step 1: Write failing focus and discoverability tests**

Assert left-outline click changes section, `Tab`/`Shift+Tab` traverse Outline → Canvas → Inspector, `?` describes the selected Canvas and current gestures, Escape cancels a field before leaving focus, and clicking blank Canvas in Experiment never Applies.

- [ ] **Step 2: Write failing footer width tests**

For every width `20..=240`, render both rows and assert each row's Unicode display width is at most the allocated width and no priority segment is partially emitted.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib workbench::action tui::workbench`

- [ ] **Step 4: Implement typed routing and split TUI modules**

Route local and C/S crossterm events through the same `WorkbenchController`. Workbench draft actions stay local until Apply; their receipt correlates UI sequence to draft/scene generation. Simulation inputs retain server input acknowledgements. Render visible focus borders, selected outline rows, tool labels, draft status, and context help.

- [ ] **Step 5: Run GREEN and commit**

Run: `cargo test --lib workbench tui::workbench input`

```bash
git add src/workbench src/tui src/input.rs src/app.rs
git commit -m "feat: make workbench navigation discoverable"
```

### Task 3: Topological-neighbor Tiling editor

**Files:**
- Modify: `src/workbench/tiling_editor.rs`
- Create: `src/tui/workbench/tiling.rs`
- Modify: `src/workbench/action.rs`
- Modify: `src/workbench/state.rs`

**Interfaces:**
- Produces: `TilingTool::{Select, DrawPolygon, AddNeighbor, ConfirmSeam, SplitEdge, Pan}`.
- Produces: `TilingScene::from_validation(draft, selected_basis, report, camera, construction) -> Self`.
- Consumes: `TilingValidationReport::neighbor_ring` and diagnostic provenance from the model plan.

- [ ] **Step 1: Write failing construction-controller tests**

Test click-to-place, pointer preview, close by first vertex/double-click/Enter, vertex drag, remove, numeric vertex edit, Undo/Redo, middle pan, empty wheel zoom, full-edge snap, interior T snap, confirm/cancel suggestion, and selection of a ghost neighbor mapping back to its basis.

- [ ] **Step 2: Write failing semantic pixel tests**

Render a regular hexagon and assert six ghosted neighbors surround one opaque editable center at the expected angular sectors. Render octagon-square and assert non-axis-aligned octagon seams and square neighbors. Assert confirmed, suggested, atomic-T, gap, overlap, and crossing colors occupy their referenced edge regions.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib workbench::tiling_editor tui::workbench::tiling`

- [ ] **Step 4: Implement the central-only editable scene**

Remove full-viewport indiscriminate repetition. Draw the selected canonical basis strongly, one arrangement-derived neighbor ring at reduced alpha, lattice vectors and patch boundary, then provenance-linked overlays and handles. Reserve all raster work from one global edge/pixel budget before drawing.

- [ ] **Step 5: Implement visible tool flow and numeric controls**

The canvas header contains tool buttons and a one-line next-action hint. The Inspector shows basis/prototype identity, vertices, lattice vectors, seam counts, coverage, Euler value, and selected diagnostic. Enter/E opens the selected vertex or vector numeric field with cursor/commit/cancel.

- [ ] **Step 6: Run GREEN and commit**

Run: `cargo test --lib workbench::tiling_editor tui::workbench::tiling sim::tiling`

```bash
git add src/workbench/tiling_editor.rs src/workbench/action.rs src/workbench/state.rs src/tui/workbench/tiling.rs
git commit -m "feat: visually construct periodic basis tilings"
```

### Task 4: Basis-aware floating-point Kernel editor

**Files:**
- Modify: `src/workbench/kernel_editor.rs`
- Create: `src/workbench/numeric_editor.rs`
- Create: `src/tui/workbench/kernel.rs`
- Modify: `src/workbench/action.rs`
- Modify: `src/workbench/state.rs`

**Interfaces:**
- Produces: `KernelSelection { offset: [i16; 2], source_basis: BasisId }`.
- Produces: `NumericEditor::begin(label, original, range)`, `edit(TextAction)`, `commit() -> Result<f64, NumericError>`, and `cancel()`.
- Produces: `KernelAdjustment::{Normal, Fine, Coarse}` mapped to configurable steps.

- [ ] **Step 1: Write failing float and exact-entry tests**

Select one polygon and assert wheel changes `0.0 → 0.05`, Shift-wheel changes by `0.005`, Ctrl-wheel changes by `0.5`, negative values work, non-finite input is rejected, Escape restores the original, and Enter commits the exact typed decimal.

- [ ] **Step 2: Write failing reachability and geometry-render tests**

For a maximum stencil, center every `(offset, source_basis)` through keyboard and zoom/pan and assert a corresponding screen hit exists. For hexagon and octagon-square fixtures, assert heatmap pixels lie inside actual source polygons rather than rectangular matrix cells.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib workbench::numeric_editor workbench::kernel_editor tui::workbench::kernel`

- [ ] **Step 4: Implement actual-tiling heatmap and gestures**

Translate each lattice offset, draw every enabled source basis polygon with signed heat color, outline the target basis, and overlay mask/anchor/selection. Use the shared transform for hit-testing. Drag paints; secondary drag clears or opens context; middle drag pans; empty wheel zooms.

- [ ] **Step 5: Implement metadata and RuleSet controls**

Show basis/channel target, source channel, enabled source bases, kernel list/count, stable symbol, extent, anchor, normalization, mask/symmetry, range, paint value, and steps. Add/Remove updates growth arity atomically. Show inherited/default/shared/local state with Detach, Edit default, and Reset to default actions.

- [ ] **Step 6: Run GREEN and commit**

Run: `cargo test --lib workbench::numeric_editor workbench::kernel_editor tui::workbench::kernel workbench::state`

```bash
git add src/workbench src/tui/workbench/kernel.rs
git commit -m "feat: edit basis kernel weights precisely"
```

### Task 5: Central Growth source editor and precise plots

**Files:**
- Modify: `src/workbench/text_buffer.rs`
- Modify: `src/workbench/growth_editor.rs`
- Replace internals: `src/workbench/growth_graph.rs`
- Create: `src/tui/workbench/growth.rs`
- Modify: `src/sim/growth/plot.rs`
- Modify: `src/workbench/action.rs`

**Interfaces:**
- Produces: `TextSelection { anchor, cursor }`, span styles, viewport line/column, word movement, and source-revision-aware diagnostics.
- Produces: `GrowthPlotMode::{Curve { axis }, Heatmap { x_axis, y_axis }}` and pinned values for every remaining kernel input, parameter, and `self`.
- Signature: `fn growth(self: Scalar, <stable kernel symbols...>) -> Rate`, with target basis/channel displayed separately.

- [ ] **Step 1: Write failing UTF-8 editor interaction tests**

Cover mouse cursor placement, Shift selection, word movement, Home/End, multiline insertion, Backspace/Delete, scrolling, Undo/Redo, bracket matching, line-number width, and diagnostic span after a Unicode comment.

- [ ] **Step 2: Write failing final-source and plot tests**

Type a source whose prefixes are valid but final text is invalid and assert no fresh valid plot generation. Fix the final character and assert diagnostics clear and plot source revision equals the final buffer revision. Assert a two-input request yields a nonuniform heatmap, contours, axes, and a selected sample readout.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib workbench::text_buffer workbench::growth_editor workbench::growth_graph sim::growth::plot`

- [ ] **Step 4: Implement central split layout**

Render target and generated read-only signature at the top, the real styled source editor in the upper central region, and the RGBA plot in the lower central region. Inspector tabs contain Rule, Params, and Plot controls only. A debounce worker is generation-cancellable and publishes only results whose source revision still matches.

- [ ] **Step 5: Implement curve/heatmap sampling and hover trace**

Curve uses one selected axis; heatmap uses two; all other inputs are explicit pinned controls. Draw pixel-space axes, ticks, grid, zero contour, invalid samples, legend, cursor/crosshair, and stale overlay. Preserve the last valid pixels on error but never count their stale decoration as a new valid plot.

- [ ] **Step 6: Run GREEN and commit**

Run: `cargo test --lib workbench::text_buffer workbench::growth_editor workbench::growth_graph sim::growth::plot tui::workbench::growth`

```bash
git add src/workbench src/sim/growth/plot.rs src/tui/workbench/growth.rs
git commit -m "feat: edit and plot complete growth programs"
```

### Task 6: Channel editor and shared Kitty/half-block behavior

**Files:**
- Modify: `src/render/channels.rs`
- Modify: `src/workbench/channel_editor.rs`
- Create: `src/tui/workbench/channels.rs`
- Modify: `src/tui/workbench/mod.rs`
- Modify: `src/app.rs`
- Modify: `tests/workbench_e2e.rs`

**Interfaces:**
- Produces: automatic palette function `channel_palette(count) -> Vec<RgbColor>` and persistent custom color edits.
- All editor actions consume `WorkbenchAction`; transport selection changes presentation only.

- [ ] **Step 1: Add failing palette and background tests**

Assert one channel is near-white, exactly three are RGB in stable channel order, Custom survives count changes, in-domain zero is `[0,0,0]`, exterior is the existing navy constant, and Inspect returns exact unquantized values.

- [ ] **Step 2: Add failing dual-transport interaction tests**

Feed the same action sequence to Kitty and half-block controllers: navigate, draw, float-wheel, exact numeric edit, multiline text edit, pan, zoom, Apply intent, and exit. Assert identical draft and camera states and no transport-specific action branch.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib render::channels workbench::channel_editor && cargo test --test workbench_e2e`

- [ ] **Step 4: Implement Channels central visuals and precise color controls**

Render Composite/Solo/Grid, visible/frozen indicators, explicit Add/Remove, color swatches and exact RGB fields. Adding a channel creates one default one-kernel RuleSet and inherited bindings for every basis; removing it removes only its bindings/default/set objects when unreferenced.

- [ ] **Step 5: Remove transport-specific editor input paths and verify graphics cleanup**

Kitty and half-block call the same controller. Resize increments transform generation once. Section and mode changes synchronously queue deletion of the presented placement before text or another graphic occupies the canvas.

- [ ] **Step 6: Run full remote checks and commit**

Run: `cargo test --lib && cargo test --test workbench_e2e && cargo test --test remote_e2e -- --skip tinker`

```bash
git add src tests/workbench_e2e.rs
git commit -m "feat: complete transport-independent visual workbench"
```

Expected final boundary: every approved editor operation is visible, discoverable, reversible, and accessible by real mouse and keyboard in both render transports; multi-basis simulation execution is added in the next plan.

