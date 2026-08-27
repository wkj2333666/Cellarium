# Visual Workbench and Remote End-to-End Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the complete outline-first terminal workbench, polygon/channel rendering, Growth editor and plots, responsive footer, remote visual subscriptions, and reliable hybrid end-to-end coverage.

**Architecture:** A `WorkbenchState` owns the mutable draft, semantic Undo/Redo stack, focus, tools, and lightweight previews; `ExperimentService` remains the only Apply authority. Editors emit semantic commands independent of terminal events. Polygon rendering uses a latest-only cached pixel-to-tile map, while C/S mode receives only quantized values for subscribed channels and requests exact Inspect values separately.

**Tech Stack:** Rust 2024, Ratatui 0.30, Crossterm 0.29, Kitty graphics/shared memory, existing latest-only workers, protocol PTY probes, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-21-interactive-experiment-workbench-design.md`

## Global Constraints

- Simulation mode and previous direct rendering remain available.
- Kitty or another supported graphics terminal defaults to graphics rendering; explicit fallback modes remain available.
- The footer is exactly two rows and removes whole low-priority segments rather than cutting glyphs.
- New experiments start with one channel; exactly three automatic channels use red, green, and blue.
- In-domain zero is pure black; out-of-domain pixels retain dark navy.
- Every mouse operation has a keyboard/numeric alternative.
- Draft edits preview locally but affect runtime only through `Ctrl+Enter` Apply.
- C/S input acknowledgement remains authoritative; optimistic local UI state is not accepted as E2E proof.
- Performance acceptance runs on tinker/NVIDIA, never from local ARM64 simulation or geometry throughput.

---

### Task 1: Responsive two-row footer and Workbench shell

**Files:**
- Create: `src/workbench/mod.rs`
- Create: `src/workbench/state.rs`
- Create: `src/tui/workbench.rs`
- Modify: `src/lib.rs`
- Modify: `src/app.rs`
- Modify: `src/tui/mod.rs`
- Test: `src/tui/mod.rs`
- Test: `src/tui/workbench.rs`

**Interfaces:**
- Produces: `AppMode::{Simulation,Workbench}`, `WorkbenchSection`, `WorkbenchFocus`, `DraftStatus`, `WorkbenchState`.
- Produces: `footer_segments(&App, width) -> [Vec<StatusSegment>; 2]`, `draw_workbench`.

- [ ] **Step 1: Add terminal-width and shell layout tests**

```rust
#[test]
fn footer_is_two_rows_and_every_segment_fits_at_supported_widths() {
    for width in [60, 80, 120, 200] {
        let rows = footer_segments(&app_fixture(), width);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| display_width(row) <= width as usize));
        assert!(rows.iter().flatten().all(|segment| !segment.text.ends_with('\u{fffd}')));
    }
}

#[test]
fn wide_workbench_uses_outline_canvas_inspector_and_narrow_hides_inspector() {
    assert_eq!(workbench_layout(Rect::new(0, 0, 180, 50)).regions().len(), 3);
    assert_eq!(workbench_layout(Rect::new(0, 0, 80, 30)).regions().len(), 2);
}
```

- [ ] **Step 2: Run layout tests before creating the shell**

Run: `cargo test --locked --lib tui::tests::footer_is_two_rows && cargo test --locked --lib tui::workbench`

Expected: new tests fail because the footer still concatenates status/commands and Workbench is absent.

- [ ] **Step 3: Split footer data from rendering**

Define `StatusSegment { text, style, priority }`. Build row one from mode/run/tick/selection/draft and row two from focus-specific commands plus Help. Repeatedly remove the highest numeric-priority optional segment until Unicode display width fits; never substring a segment. Statistics remain in the inspector and retain distinct labels for server simulation, snapshot/visual receive, UI draw, fresh graphics, and Kitty consume where observable; configured targets are labeled `target`.

- [ ] **Step 4: Add outline-first Workbench layout**

At width at least 120 allocate `24` columns to outline, at least `60` to canvas, and `36` to inspector. Below that threshold keep outline plus canvas and open Inspector as an overlay. Render stable sections `World`, `Tiling`, `Channels`, `Kernels`, `Growth`, and `Experiment`; do not expose editing operations until their tasks land.

- [ ] **Step 5: Run TUI tests**

Run: `cargo test --locked --lib tui`

Expected: all shell/footer tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/workbench src/tui/workbench.rs src/lib.rs src/app.rs src/tui/mod.rs
git commit -m "feat: add responsive experiment workbench shell"
```

### Task 2: Semantic draft commands, focus, and Undo/Redo

**Files:**
- Create: `src/workbench/command.rs`
- Create: `src/workbench/history.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/input.rs`
- Modify: `src/app.rs`
- Test: `src/workbench/history.rs`
- Test: `src/input.rs`

**Interfaces:**
- Produces: `DraftCommand`, `CommandEffect`, `History`, `WorkbenchState::execute`, `undo`, `redo`, `apply`, `revert`.
- Produces: `UiCommand::{EnterWorkbench,LeaveWorkbench,ApplyDraft,RevertDraft,Undo,Redo,FocusNext,FocusPrevious}`.

- [ ] **Step 1: Add reversible command and transaction tests**

```rust
#[test]
fn paint_command_roundtrips_through_undo_and_redo() {
    let mut state = workbench_fixture();
    let before = state.draft().clone();
    state.execute(DraftCommand::SetChannelValue { channel: ChannelId(0), tile: 3, value: 1.0 }).unwrap();
    assert_ne!(state.draft(), &before);
    state.undo().unwrap();
    assert_eq!(state.draft(), &before);
    state.redo().unwrap();
    assert_eq!(state.draft().channels[0].initial[3], 1.0);
}

#[test]
fn apply_key_is_ctrl_enter_and_plain_enter_edits_current_control() {
    assert_eq!(translate_ui_key(ctrl(KeyCode::Enter)), Some(UiCommand::ApplyDraft));
    assert_ne!(translate_ui_key(key(KeyCode::Enter)), Some(UiCommand::ApplyDraft));
}
```

- [ ] **Step 2: Run input/history tests before implementation**

Run: `cargo test --locked --lib workbench::history && cargo test --locked --lib input`

Expected: compile failure for missing workbench commands.

- [ ] **Step 3: Implement semantic inverse commands**

Each `DraftCommand::apply(&mut ExperimentSpec)` returns its exact inverse containing prior values/objects. History stores at most 1024 command pairs, clears redo on new edits, and coalesces consecutive paint/drag updates within one gesture. Apply/Revert are not inserted into Undo history. Revert replaces the draft with the last authoritative normalized spec and clears history.

- [ ] **Step 4: Route keys and mouse by mode/focus**

Keep Simulation key behavior intact. In Workbench, `Tab`/`Shift+Tab` move focus, `Ctrl+Enter` submits, `Ctrl+Z`/`Ctrl+Y` undo/redo, `Esc` cancels the current field then leaves Workbench only when no edit is active, and `?` opens context help. In C/S mode semantic draft commands remain local; only complete Apply, visual subscription, and Inspect messages cross the protocol. Local draft changes never masquerade as authoritative server state.

- [ ] **Step 5: Run workbench/input/App tests**

Run: `cargo test --locked --lib workbench && cargo test --locked --lib input && cargo test --locked --lib app`

Expected: all focused tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/workbench/command.rs src/workbench/history.rs src/workbench/state.rs src/input.rs src/app.rs
git commit -m "feat: edit experiment drafts with undo and redo"
```

### Task 3: Channel colors, Composite/Solo/Grid, and initial-field editor

**Files:**
- Create: `src/render/channels.rs`
- Create: `src/workbench/channel_editor.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `src/sim/experiment_model.rs`
- Test: `src/render/channels.rs`
- Test: `src/workbench/channel_editor.rs`

**Interfaces:**
- Consumes: model-owned `ChannelDisplay`, `DisplayColor`, and `RgbColor` from the Foundation plan.
- Produces: `ChannelView::{Composite,Solo,Grid}`, `automatic_palette`, `composite_pixel`, and lossless conversions between model `RgbColor` and render `Rgb8`.
- Produces channel commands: add/remove/rename/freeze/color/visibility/select/paint/fill/clear.

- [ ] **Step 1: Add exact color and blend tests**

```rust
#[test]
fn automatic_palette_obeys_one_and_three_channel_defaults() {
    assert_eq!(automatic_palette(1), vec![Rgb8::new(245, 245, 245)]);
    assert_eq!(automatic_palette(3), vec![
        Rgb8::new(255, 0, 0), Rgb8::new(0, 255, 0), Rgb8::new(0, 0, 255),
    ]);
}

#[test]
fn domain_zero_is_black_and_exterior_is_navy() {
    assert_eq!(composite_pixel(&[0.0], &[Rgb8::new(245,245,245)]), Rgb8::new(0,0,0));
    assert_eq!(OUTSIDE_DOMAIN, Rgb8::new(8,12,24));
}

#[test]
fn custom_color_survives_channel_count_change() {
    let mut editor = channel_editor_fixture();
    editor.set_custom_color(ChannelId(0), Rgb8::new(1,2,3)).unwrap();
    editor.add_channel("B").unwrap();
    assert_eq!(editor.resolved_color(ChannelId(0)), Rgb8::new(1,2,3));
}
```

- [ ] **Step 2: Run channel tests before implementation**

Run: `cargo test --locked --lib render::channels && cargo test --locked --lib workbench::channel_editor`

Expected: compile failure for missing channel presentation modules.

- [ ] **Step 3: Implement bounded screen compositing**

For normalized value `v` and channel color component `c`, accumulate in linearized byte space with `out = 1 - product(1 - v*c)`, then convert to `u8`. Custom colors stay pinned; Auto colors resolve from visible automatic channels. Grid computes deterministic sub-view rectangles and Solo retains the selected channel's configured color with an optional grayscale toggle.

- [ ] **Step 4: Add channel editor and initial-field painting**

Render channel rows with color swatch, visibility, frozen state, and selected marker. Paint modifies only the selected channel/tile and updates Composite immediately. Removing a referenced channel is a blocking confirmation listing affected kernels/growth programs; the semantic command removes or reroutes only after explicit choice.

- [ ] **Step 5: Run render/editor/model tests**

Run: `cargo test --locked --lib render::channels && cargo test --locked --lib workbench::channel_editor && cargo test --locked --lib sim::experiment_model`

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/render/channels.rs src/render/mod.rs src/workbench/channel_editor.rs src/workbench/mod.rs src/tui/workbench.rs src/sim/experiment_model.rs
git commit -m "feat: visualize and edit simulation channels"
```

### Task 4: Multi-line Growth source editor and live plots

**Files:**
- Create: `src/workbench/text_buffer.rs`
- Create: `src/workbench/growth_editor.rs`
- Create: `src/render/plot.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/text_buffer.rs`
- Test: `src/workbench/growth_editor.rs`
- Test: `src/render/plot.rs`

**Interfaces:**
- Produces: `TextBuffer`, `GrowthEditorState`, `CompletionItem`, `PlotViewport`, `render_plot`.
- Consumes: typed Growth diagnostics, `sample_plot`, traces, and operating summaries from Plan 2.

- [ ] **Step 1: Add cursor/edit/diagnostic/plot tests**

```rust
#[test]
fn text_buffer_edits_on_utf8_boundaries_but_growth_identifiers_remain_ascii() {
    let mut buffer = TextBuffer::new("// café\ninner");
    buffer.move_end();
    buffer.insert_str(" + self");
    assert_eq!(buffer.as_str(), "// café\ninner + self");
    assert!(buffer.cursor_is_char_boundary());
}

#[test]
fn invalid_source_keeps_last_valid_plot_marked_stale() {
    let mut editor = growth_editor_fixture("inner");
    editor.refresh_now();
    let valid = editor.plot().clone();
    editor.replace_source("if inner {");
    editor.refresh_now();
    assert_eq!(editor.plot().data, valid.data);
    assert!(editor.plot().stale);
    assert!(!editor.diagnostics().is_empty());
}
```

- [ ] **Step 2: Run editor/plot tests before implementation**

Run: `cargo test --locked --lib workbench::text_buffer && cargo test --locked --lib workbench::growth_editor && cargo test --locked --lib render::plot`

Expected: compile failure for missing modules.

- [ ] **Step 3: Implement a real source editor**

Store source as `String` plus byte cursor/selection constrained to character boundaries. Support insert, newline, backspace/delete, selection replacement, Home/End, vertical movement by display column, bracket matching, local Undo, and span styling. `Ctrl+Space` completion lists routed kernel symbols, `self`, parameters, locals in scope, constants, and whitelisted functions.

- [ ] **Step 4: Debounce and cancel parse/plot work**

Assign a monotonic edit generation. After 100 ms without an edit, parse/type/lint and enqueue the latest plot request; older generations abort per plot row. Keep last valid plot with a visible `STALE` marker when current source is invalid. Render curves, heatmaps, zero contours, runtime histograms/density, hover crosshair, and trace table using the existing pixel viewport.

- [ ] **Step 5: Expose generated signature and parameters**

Show the read-only `growth_<target>(kernel symbols; self) -> rate|next` line. Inspector supports parameter add/remove/name/value/range/linear-or-log scale and plot X/Y/pinned choices. It cannot manually alter kernel input cardinality.

- [ ] **Step 6: Run Growth UI tests**

Run: `cargo test --locked --lib workbench::growth_editor && cargo test --locked --lib workbench::text_buffer && cargo test --locked --lib render::plot && cargo test --locked --lib sim::growth`

Expected: all focused tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/workbench/text_buffer.rs src/workbench/growth_editor.rs src/render/plot.rs src/workbench/mod.rs src/tui/workbench.rs
git commit -m "feat: edit and plot structured growth programs"
```

### Task 5: Interactive periodic tiling editor

**Files:**
- Create: `src/workbench/tiling_editor.rs`
- Create: `src/render/tiling_overlay.rs`
- Modify: `src/workbench/command.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/tiling_editor.rs`
- Test: `src/render/tiling_overlay.rs`

**Interfaces:**
- Produces: `TilingTool::{Select,Place,Move,Rotate,SnapEdge,Pan}`, `TilingSelection`, picking APIs, tiling semantic commands, validation overlays.

- [ ] **Step 1: Add place/snap/undo and overlay tests**

```rust
#[test]
fn placing_and_snapping_square_to_octagon_is_one_undo_gesture() {
    let mut editor = octagon_editor_fixture();
    editor.begin_place(square_prototype());
    editor.pointer_down(screen(40, 20));
    editor.pointer_drag(screen(52, 20));
    editor.pointer_up(screen(52, 20));
    assert_eq!(editor.draft().tiling.instances.len(), 2);
    assert!(editor.validation().unmatched_edges < editor.validation_before().unmatched_edges);
    editor.undo().unwrap();
    assert_eq!(editor.draft().tiling.instances.len(), 1);
}

#[test]
fn overlay_colors_distinguish_overlap_gap_unmatched_and_valid_edges() {
    let overlay = overlay_for(invalid_overlay_fixture());
    assert!(overlay.primitives.iter().any(|p| p.style == OverlayStyle::OverlapRed));
    assert!(overlay.primitives.iter().any(|p| p.style == OverlayStyle::GapPurple));
    assert!(overlay.primitives.iter().any(|p| p.style == OverlayStyle::UnmatchedOrange));
}
```

- [ ] **Step 2: Run tiling editor tests before implementation**

Run: `cargo test --locked --lib workbench::tiling_editor && cargo test --locked --lib render::tiling_overlay`

Expected: compile failure for missing editor/overlay.

- [ ] **Step 3: Implement real-space picking and gestures**

Convert terminal mouse cells through camera to `f64` world coordinates. Pick selected face by point containment and edge by bounded screen-space distance. Place, translate, rotate, and SnapEdge gestures emit one coalesced semantic command on pointer-up. Keyboard alternatives move by grid step, rotate by configured angle, cycle edges, and edit exact numeric fields.

- [ ] **Step 4: Render periodic copies and diagnostics**

Show the editable representatives strongly and bounded surrounding copies dimly. Independent toggles display polygons, translations, seams, derived graph, fundamental patch, and expanded domain. Render overlaps red, gaps purple, unmatched edges orange, paired edges green, and selection in focus blue. Never label the parallelogram as the polygon cell shape.

- [ ] **Step 5: Add prototype/preset/translation Inspector controls**

Support Regular Polygon with side count/length, Custom Polygon vertex table and a DrawPolygon click tool, instance rotation/translation, Detach, Delete, translations `a/b`, and Square/Hex/Honeycomb/OctagonSquare presets. `Fill Gap` creates a visibly provisional polygon suggestion that enters the draft only after confirmation. `Suggest Period` may populate a preview; only explicit Confirm changes translations.

- [ ] **Step 6: Run tiling and geometry tests**

Run: `cargo test --locked --lib workbench::tiling_editor && cargo test --locked --lib render::tiling_overlay && cargo test --locked --lib sim::tiling`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/workbench/tiling_editor.rs src/render/tiling_overlay.rs src/workbench/command.rs src/workbench/mod.rs src/tui/workbench.rs
git commit -m "feat: edit periodic polygon tilings interactively"
```

### Task 6: World, domain, and multi-kernel visual editors

**Files:**
- Create: `src/workbench/world_editor.rs`
- Create: `src/workbench/kernel_editor.rs`
- Modify: `src/workbench/command.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/world_editor.rs`
- Test: `src/workbench/kernel_editor.rs`

**Interfaces:**
- Produces: domain paint/fill/resize/boundary commands; kernel add/remove/route/paint/formula/cutoff/normalization commands.

- [ ] **Step 1: Add polygon-domain and routing/cardinality tests**

```rust
#[test]
fn domain_brush_uses_picked_polygon_tile_not_screen_grid_cell() {
    let mut editor = mixed_tiling_world_editor();
    let tile = editor.pick(screen_over_small_square()).unwrap();
    editor.erase_at(screen_over_small_square()).unwrap();
    assert!(!editor.domain().contains(tile)); // `tile` is a stable TileAddress
}

#[test]
fn rerouting_kernel_moves_exactly_one_growth_input() {
    let mut editor = two_channel_kernel_editor();
    let kernel = editor.selected_kernel_id();
    editor.route(kernel, ChannelId(0), ChannelId(1)).unwrap();
    assert!(!editor.growth(ChannelId(0)).kernel_inputs.contains(&kernel));
    assert!(editor.growth(ChannelId(1)).kernel_inputs.contains(&kernel));
}
```

- [ ] **Step 2: Run editor tests before implementation**

Run: `cargo test --locked --lib workbench::world_editor && cargo test --locked --lib workbench::kernel_editor`

Expected: compile failure for missing modules.

- [ ] **Step 3: Implement World editing on actual tiles**

Provide pencil, erase, rectangle-in-world-space, connected fill, resize/repeat-range, mask/sparse modes, and boundary preview. Constant boundary values are editable per channel with an all-channel broadcast action. Paint state and domain membership are separate tools.

- [ ] **Step 4: Implement multi-kernel editing**

List kernels by stable symbol and `source -> target`. Add/remove/reroute updates target growth bindings atomically. Raster kernels show heatmap/mask/anchor; topological kernels show hop profile; spatial kernels show distance profile/cutoff and selected target contributors. Provide paint, erase, fill, symmetry, normalize, explicit/formula mode, and parameter controls.

- [ ] **Step 5: Run editor/model/compiler tests**

Run: `cargo test --locked --lib workbench::world_editor && cargo test --locked --lib workbench::kernel_editor && cargo test --locked --lib sim::experiment_model && cargo test --locked --lib sim::tiling::kernel_weights`

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/workbench/world_editor.rs src/workbench/kernel_editor.rs src/workbench/command.rs src/workbench/mod.rs src/tui/workbench.rs
git commit -m "feat: edit domains and routed kernels visually"
```

### Task 7: Experiment review, load, save, and export

**Files:**
- Create: `src/workbench/experiment_editor.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `src/sim/experiment.rs`
- Test: `src/workbench/experiment_editor.rs`
- Test: `src/sim/experiment.rs`

**Interfaces:**
- Produces: `ExperimentEditorState`, `load_draft`, `export_draft`, `save_active`, compatibility/backend review rows.

- [ ] **Step 1: Add non-destructive load and invalid-draft export tests**

```rust
#[test]
fn malformed_load_preserves_current_draft_and_history() {
    let mut editor = experiment_editor_fixture();
    let before = editor.audit_draft();
    assert!(editor.load_bytes(b"not ron").is_err());
    assert_eq!(editor.audit_draft(), before);
}

#[test]
fn export_draft_preserves_invalid_source_without_applying_it() {
    let mut editor = experiment_editor_fixture();
    editor.replace_growth_source(ChannelId(0), "if inner {");
    let bytes = editor.export_draft_bytes().unwrap();
    let loaded = decode_draft_envelope(&bytes).unwrap();
    assert_eq!(loaded.growth[0].source, "if inner {");
    assert_eq!(editor.active_revision(), 0);
}

#[test]
fn save_active_uses_authoritative_normalized_experiment() {
    let editor = experiment_editor_after_apply();
    let saved = decode_experiment_model(&editor.save_active_bytes().unwrap()).unwrap();
    assert_eq!(saved, editor.snapshot_active_experiment());
}
```

- [ ] **Step 2: Run experiment editor tests before implementation**

Run: `cargo test --locked --lib workbench::experiment_editor && cargo test --locked --lib sim::experiment`

Expected: compile failure for the missing editor and draft envelope.

- [ ] **Step 3: Separate authoritative experiment files from recovery drafts**

`save_active` writes the normal validated versioned experiment. `export_draft` writes `DraftEnvelope { format_version, base_revision, draft }` after bounded structural/size checks but permits semantic Growth/tiling diagnostics so work is recoverable. `load_draft` parses and migrates into Workbench without applying; `load_active_as_draft` imports a valid experiment the same way. Parse/migration failure leaves draft, selection, and history untouched.

- [ ] **Step 4: Render Experiment review and local file actions**

Show metadata, base/active revision, dirty sections, all diagnostics grouped by object path, backend choice, estimated channel/tile/edge counts, and compatibility. Provide Load as Draft, Save Active, Export Draft, Apply, and Revert. In C/S mode file paths are local-client paths because the client owns persistence UI; Export Draft is entirely local, while Save Active requests one exact authoritative experiment snapshot from tinker and writes the response locally.

- [ ] **Step 5: Run persistence/editor tests**

Run: `cargo test --locked --lib workbench::experiment_editor && cargo test --locked --lib sim::experiment && cargo test --locked --test workflow_contract`

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/workbench/experiment_editor.rs src/workbench/mod.rs src/tui/workbench.rs src/sim/experiment.rs
git commit -m "feat: review and persist experiment drafts"
```

### Task 8: Cached polygon raster map and latest-only rendering

**Files:**
- Create: `src/render/tile_map.rs`
- Create: `src/render/polygon_raster.rs`
- Modify: `src/render/raster.rs`
- Modify: `src/render/display/mod.rs`
- Modify: `src/tui/mod.rs`
- Test: `src/render/tile_map.rs`
- Test: `src/render/polygon_raster.rs`

**Interfaces:**
- Produces: `TileRasterKey`, `TileRasterMap`, `TileMapWorker`, `build_tile_map`, `rasterize_channels_into`; map entries are dense compiled-domain indices whose `TileAddress` table is revision-scoped metadata.
- Preserves: existing display protocol selection and Kitty shared-memory presentation/deletion behavior.

- [ ] **Step 1: Add map reuse, cancellation, and background tests**

```rust
#[test]
fn unchanged_camera_and_geometry_reuse_tile_map_across_state_frames() {
    let mut cache = TileMapCache::default();
    let first = cache.get_or_build(key(1, 2), geometry(), camera(), size(320, 200));
    let second = cache.get_or_build(key(1, 2), geometry(), camera(), size(320, 200));
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn latest_only_worker_discards_obsolete_camera_generation() {
    let worker = TileMapWorker::new();
    worker.request(request_with_generation(1));
    worker.request(request_with_generation(2));
    assert_eq!(worker.wait_ready().generation, 2);
}

#[test]
fn map_distinguishes_black_domain_zero_from_navy_exterior() {
    let frame = raster_fixture_with_hole();
    assert_eq!(frame.get(domain_zero_pixel()), Rgb8::new(0,0,0));
    assert_eq!(frame.get(exterior_pixel()), Rgb8::new(8,12,24));
}
```

- [ ] **Step 2: Run raster tests before implementation**

Run: `cargo test --locked --lib render::tile_map && cargo test --locked --lib render::polygon_raster`

Expected: compile failure for missing raster modules.

- [ ] **Step 3: Build and cache pixel-to-tile maps**

Rasterize compiled triangles into `Vec<Option<u32>>` using pixel-center coverage and deterministic edge ownership; use supersampled coverage only at fractional zoom where needed. Key by geometry revision, camera transform, viewport pixels, and Grid sub-view. Check cancellation per row. Cache only the newest complete map.

- [ ] **Step 4: Composite fresh channel values through the map**

For each pixel, `None` writes navy; `Some(tile)` gathers subscribed channel values and calls the channel compositor. Reuse Framebuffer allocation. Preserve latest-only async behavior so input processing never waits for map rebuild or graphics encoding.

- [ ] **Step 5: Run render/display tests**

Run: `cargo test --locked --lib render && cargo test --locked --lib tui && cargo test --locked --test pty_startup`

Expected: all render, Kitty command/lifetime, and PTY tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/render/tile_map.rs src/render/polygon_raster.rs src/render/raster.rs src/render/display/mod.rs src/tui/mod.rs
git commit -m "feat: render polygon channels through cached tile maps"
```

### Task 9: Remote visual subscriptions, Growth summaries, and exact Inspect

**Files:**
- Modify: `src/remote.rs`
- Modify: `src/app.rs`
- Modify: `src/workbench/state.rs`
- Modify: `tests/support/remote_probe.rs`
- Test: `src/remote.rs`
- Test: `src/app.rs`

**Interfaces:**
- Produces messages: `VisualSubscription`, `VisualFrame`, `GrowthSummaryRequest`, `GrowthSummaryResponse`, `InspectRequest`, `InspectResponse`, `ExportActiveRequest`, `ExportActiveResponse`.
- Produces: `QuantizedPlane { channel, values: Vec<u8> }`; exact Inspect accepts `TileAddress` and returns `Vec<(ChannelId, f32)>`.

- [ ] **Step 1: Add subscription, quantization, and exactness tests**

```rust
#[test]
fn visual_frame_contains_only_subscribed_channels() {
    let server = three_channel_server();
    let frame = server.visual_frame(&[ChannelId(0), ChannelId(2)]).unwrap();
    assert_eq!(frame.planes.iter().map(|p| p.channel).collect::<Vec<_>>(), vec![ChannelId(0), ChannelId(2)]);
}

#[test]
fn inspect_is_exact_even_when_visual_plane_is_quantized() {
    let server = server_with_value(0.1234567);
    let visual = server.visual_frame(&[ChannelId(0)]).unwrap();
    assert_ne!(visual.planes[0].values[0] as f32 / 255.0, 0.1234567);
    assert_eq!(server.inspect(TileAddress::origin(TileId(0))).values[0].1, 0.1234567);
}

#[test]
fn growth_summary_is_bounded_and_revision_scoped() {
    let response = three_channel_server().growth_summary(GrowthSummaryRequest {
        request_id: 8,
        revision: 0,
        target: ChannelId(1),
        x_symbol: "inner".into(),
        y_symbol: Some("outer".into()),
        bins: 32,
    }).unwrap();
    assert_eq!(response.revision, 0);
    assert_eq!(response.summary.density.len(), 32 * 32);
}

#[test]
fn reconnect_conflict_preserves_local_draft_until_explicit_choice() {
    let mut state = disconnected_modified_workbench(base_revision(3));
    let local = state.draft().clone();
    state.observe_authoritative_revision(4);
    assert_eq!(state.draft(), &local);
    assert_eq!(state.conflict(), Some((3, 4)));
}

#[test]
fn remote_active_export_contains_exact_current_state() {
    let server = stepped_three_channel_server();
    let response = server.export_active(ExportActiveRequest { request_id: 9, revision: 0 }).unwrap();
    assert_eq!(response.experiment.channels[1].initial, server.world().channel_cells(1));
}
```

- [ ] **Step 2: Run remote tests before protocol extension**

Run: `cargo test --locked --lib remote && cargo test --locked --lib app::tests::visual_frame_contains_only_subscribed_channels`

Expected: compile failure for missing messages/APIs.

- [ ] **Step 3: Add bounded latest-only visual messages**

Increment `PROTOCOL_VERSION` once. `VisualSubscription` carries a monotonic request ID and sorted unique channel IDs. `VisualFrame` carries request ID, experiment revision, geometry revision, tick, the revision-scoped ordered `TileAddress` table when geometry changes, and raw `u8` planes in that dense order; validate every `plane.len()` against the address count and the total frame cap. Convert clamped state `[0,1]` with round-to-nearest. Replace pending frames rather than queueing them. Remove the transitional repeated `Vec<f32>` cells from steady Snapshot after all clients consume `VisualFrame`; full experiment metadata remains one-shot `ExperimentState`/ApplyAccepted data.

- [ ] **Step 4: Add exact Inspect request/response**

Inspect carries input sequence, authoritative revision, and stable `TileAddress`. The server returns every channel's `f32` value or a stale-revision/unknown-tile diagnostic. The terminal probe accepts the visible result only after the response and a later consumed frame carry at least the request sequence.

- [ ] **Step 5: Add bounded latest-only Growth summaries**

Requests carry authoritative revision, target channel, one or two kernel/local symbols, and `bins` clamped to `8..=128`. The server uses Plan 2's deterministic operating sampler, returns histogram/density plus sample count, and replaces an older pending request from the same client. It never sends per-tile intermediate potentials. The Growth editor marks a response stale unless its request ID and revision match the current draft base.

- [ ] **Step 6: Preserve drafts across disconnect and revision conflict**

Disconnect changes connection state only. On reconnect, a matching revision resumes normally; a different authoritative revision sets an explicit conflict state and leaves draft/history untouched. The Experiment panel offers Export Draft, Reload Authoritative, or keep editing for later Save As. Apply remains disabled until the conflict is resolved; it never silently changes `base_revision`.

- [ ] **Step 7: Export exact active state on demand**

`ExportActiveRequest` carries request ID and expected revision. The server calls `snapshot_active_experiment`, returns the complete exact channel state once under the existing frame cap, or returns a revision/size diagnostic. This message is never part of the steady snapshot stream.

- [ ] **Step 8: Run protocol/App/probe unit tests**

Run: `cargo test --locked --lib remote && cargo test --locked --lib app && cargo test --locked --test remote_e2e -- --skip remote_protocol_e2e_on_tinker --skip remote_terminal_e2e_on_tinker`

Expected: all non-network tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/remote.rs src/app.rs src/workbench/state.rs tests/support/remote_probe.rs
git commit -m "feat: stream channel views and growth diagnostics"
```

### Task 10: Full hybrid PTY/E2E and release gates

**Files:**
- Modify: `tests/support/terminal_probe.rs`
- Modify: `tests/support/kitty_terminal.rs`
- Modify: `tests/remote_e2e.rs`
- Modify: `scripts/e2e-tinker.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `docs/remote-viewer.md`
- Create: `docs/workbench.md`

**Interfaces:**
- Produces auditable report fields for server sim, visual-frame receive, UI draw, fresh graphics, Kitty consume, input ack-to-frame, Apply, channel switch, and exact Inspect.

- [ ] **Step 1: Add fake-PTY Workbench flows**

Drive actual terminal bytes to enter Workbench, add two channels (giving three total), verify RGB Composite, edit Growth source with `let` and `if/else`, create/apply a valid square preset, reject an overlap draft, Undo/Redo a paint gesture, and Inspect a tile. Assertions must use authoritative revision/input sequence and framebuffer/Kitty content, not local optimistic fields.

- [ ] **Step 2: Run all non-network PTY tests**

Run: `cargo test --locked --test pty_startup && cargo test --locked --test remote_e2e -- --skip remote_protocol_e2e_on_tinker --skip remote_terminal_e2e_on_tinker`

Expected: all PTY and emulator tests pass.

- [ ] **Step 3: Keep CI architecture coverage**

CI must run formatting, no-default CPU tests, default tests, clippy, and Linux `aarch64-unknown-linux-gnu` check. Release jobs must continue producing locked x86_64 and aarch64 client binaries; server CUDA packaging remains on supported Linux x86_64. Add a CLI smoke that loads the checked-in octagon-square experiment without requiring a display.

- [ ] **Step 4: Install the candidate server on tinker and run mixed E2E**

Use the existing remote SSH operations workflow to build Release on tinker, verify its SHA-256, install it as `/home/wkj/.local/bin/cellarium`, and confirm `command -v cellarium` resolves that file. Then run:

```bash
CELLARIUM_E2E_HOST=tinker scripts/e2e-tinker.sh
```

Expected: NVIDIA backend; continuous server ticks; valid Apply accepted; invalid tiling rejected without revision change; keyboard/mouse acknowledgement; three-channel subscription; exact Inspect; continuously consumed Kitty shared-memory frames; and no stale-image accumulation. Report local ARM64 only as protocol/presentation client.

- [ ] **Step 5: Document actual interactions and metric meanings**

`docs/workbench.md` must cover Simulation/Workbench switching, outline navigation, Apply/Revert, channel modes/colors, tiling construction/snap/period confirmation, kernel routing, Growth syntax/plot controls, Undo/Redo, Direct/C/S behavior, and failure recovery. `docs/remote-viewer.md` must distinguish visual-frame receive, UI draw, fresh graphics, and Kitty consume rates.

- [ ] **Step 6: Run final local gates**

Run: `cargo fmt --check && cargo test --locked --no-default-features && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && cargo check --locked --target aarch64-unknown-linux-gnu && git diff --check`

Expected: every command exits zero.

- [ ] **Step 7: Commit**

```bash
git add tests/support tests/remote_e2e.rs scripts/e2e-tinker.sh .github/workflows docs
git commit -m "test: verify visual workbench end to end"
```

### Task 11: Final review and delivery

**Files:** None.

**Interfaces:** Produces a release-ready implementation of the approved spec.

- [ ] **Step 1: Request code review**

Use `superpowers:requesting-code-review` for the complete four-plan implementation range. Require explicit review of geometry lifetime/allocation bounds, draft atomicity, protocol trust boundaries, Kitty frame lifetimes, and whether UI metrics measure fresh observed events.

- [ ] **Step 2: Resolve every Critical and Important finding and rerun affected red/green tests**

For each regression fix, prove the new test fails without the fix and passes with it, then rerun the complete local and tinker gates.

- [ ] **Step 3: Use the finishing workflow**

Invoke `superpowers:finishing-a-development-branch` only after the clean-tree verification and tinker report are fresh. Present merge/push/release choices without silently changing remote state.
