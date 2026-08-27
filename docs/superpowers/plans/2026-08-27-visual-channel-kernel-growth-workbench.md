# Visual Channel, Kernel, and Growth Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not dispatch subagents; the user explicitly requires inline execution.

**Goal:** Replace the text-first Channels, Kernels, and Growth pages with discoverable graphical mouse-first editors, then certify every product feature through an observed real-user journey.

**Architecture:** Keep `WorkbenchState` as the single draft controller, but move reusable object-strip geometry, decision panels, and section view models into focused modules. Stable IDs drive every selection and hit target. Growth plotting derives axes from typed symbol use, while Kitty and half-block consume the same logical scenes.

**Tech Stack:** Rust 2024, ratatui/crossterm, Kitty graphics and half-block fallback, existing CPU/CUDA simulation backends, Xvfb/Openbox/Kitty/xdotool agentic harness, GitHub Actions release assets.

**Spec:** `docs/superpowers/specs/2026-08-27-visual-channel-kernel-growth-workbench-design.md`

## Global Constraints

- The Raspberry Pi must not compile Cellarium; all Rust builds and automated Rust tests run on `tinker`.
- Local user-level tests install only checksum-verified ARM64 binaries built by GitHub Actions.
- Default channel count is one; three channels use red, green, blue; the board background is pure black and domain exterior retains the existing dark blue.
- Default kernel count is one and additional kernels are explicit.
- Kernel values are not implicitly normalized.
- T-junctions are rejected; tilings are strict edge-to-edge.
- Existing keyboard shortcuts remain accelerators; all primary collection actions have visible mouse targets.
- No saved workspace is silently truncated, deduplicated, or reset.
- Every implementation task follows red/green/refactor and ends in a focused commit.
- A stable release is forbidden until the exact candidate artifacts pass the complete agentic contract.

## File Structure

- Create `src/workbench/object_strip.rs`: stable-ID card models, scrolling, geometry, and hit testing shared by Channels, Kernels, and Growth chips.
- Create `src/workbench/decision.rs`: persistent, keyboard/mouse-operable decision panels and action feedback.
- Create `src/workbench/growth_symbols.rs`: typed-program external-symbol usage and source rewrite helpers.
- Create `src/render/workbench_thumbnail.rs`: cached channel/kernel thumbnails and compact legends.
- Modify `src/workbench/state.rs`: persistent selections, section-specific UI state, atomic lifecycle actions, and controller methods.
- Modify `src/workbench/channel_editor.rs`: unique IDs/default names and channel card view models.
- Modify `src/workbench/kernel_editor.rs`: visual palette state, legends, exact values, and periodic/raster scene metadata.
- Modify `src/workbench/growth_editor.rs`: explicit axes, referenced-symbol defaults, pinned inputs, and plot status.
- Modify `src/workbench/growth_graph.rs`: meaningful curve/heatmap rendering, isolated samples, and empty/stale overlays.
- Modify `src/tui/workbench.rs`: section layouts, cards/chips/tabs, concise Inspector, and graphics placement.
- Modify `src/app.rs`: mouse dispatch through rendered hit regions and identical keyboard/controller routes.
- Modify `tests/workbench_e2e.rs`, `tests/workflow_contract.rs`, and in-module tests: deterministic controller/render regressions.
- Modify `tests/agentic/full-journey.md` and `docs/testing/agentic-workbench.md`: complete strict edge-to-edge user journey and evidence rules.
- Modify `tests/agentic/scripts/*.sh` only where lifecycle/evidence capture needs stronger guarantees.
- Modify `.github/workflows/ci.yml` and `.github/workflows/release.yml`: candidate artifact gate and stable publication.

---

## Phase A — Shared interaction and Channels

### Task 1: Stable object-strip layout and mouse hit testing

**Files:**
- Create: `src/workbench/object_strip.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/tui/workbench.rs:13-230`
- Test: `src/workbench/object_strip.rs`

**Interfaces:**
- Produces: `ObjectCardId`, `ObjectCard`, `ObjectStripLayout`, `ObjectStripHit`, and `layout_object_strip(cards, area, scroll)`.
- Consumes: ratatui `Rect`; card IDs remain opaque and stable.

- [ ] **Step 1: Write failing geometry tests**

```rust
#[test]
fn wrapped_strip_keeps_every_card_and_hits_delete_before_body() {
    let cards = (0..5)
        .map(|id| ObjectCard::object(ObjectCardId(id), format!("k{id}"), true))
        .chain([ObjectCard::add()])
        .collect::<Vec<_>>();
    let layout = layout_object_strip(&cards, Rect::new(10, 4, 24, 6), 0);
    assert_eq!(layout.cards.len(), 6);
    let selected = &layout.cards[2];
    assert_eq!(layout.hit(selected.delete_rect.unwrap().x, selected.delete_rect.unwrap().y),
               Some(ObjectStripHit::Delete(ObjectCardId(2))));
}
```

- [ ] **Step 2: Run the focused test on tinker and verify it fails**

Run: `cargo test --lib workbench::object_strip::tests::wrapped_strip_keeps_every_card_and_hits_delete_before_body -- --exact`

Expected: FAIL because `object_strip` does not exist.

- [ ] **Step 3: Implement the card model and pure layout**

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectCardId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectCardKind { Object(ObjectCardId), Add }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectCard {
    pub kind: ObjectCardKind,
    pub title: String,
    pub deletable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectStripHit {
    Select(ObjectCardId),
    Delete(ObjectCardId),
    Add,
}
```

Lay cards left-to-right, wrap within the supplied area, expose body/delete rectangles, and retain cards outside the viewport in logical order for scrolling.

- [ ] **Step 4: Render the strip from its layout and add narrow/wide snapshots**

Add Buffer assertions proving selected, add, hidden, frozen, and delete affordances remain visible at widths 24 and 120.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib object_strip`

Commit: `git add src/workbench/object_strip.rs src/workbench/mod.rs src/tui/workbench.rs && git commit -m "feat: add stable workbench object strips"`

### Task 2: Persistent decisions, feedback, and selections

**Files:**
- Create: `src/workbench/decision.rs`
- Modify: `src/workbench/state.rs:74-220,1120-1280`
- Modify: `src/app.rs:469-720`
- Test: `src/workbench/state.rs`

**Interfaces:**
- Produces: `DecisionPanel { title, detail, choices, selected }`, `DecisionChoice { id, label }`, `WorkbenchState::decision()`, `choose_decision(id)`, and `cancel_decision()`.
- Produces: stable-ID setters `select_channel(ChannelId)` and `select_kernel(KernelId)`.

- [ ] **Step 1: Write failing selection and decision tests**

```rust
#[test]
fn deleting_selected_object_selects_nearest_and_undo_restores_both() {
    let mut state = three_channel_state();
    state.select_channel(ChannelId(1)).unwrap();
    state.remove_selected_channel().unwrap();
    assert_eq!(state.selected_channel(), ChannelId(2));
    state.undo().unwrap();
    assert_eq!(state.selected_channel(), ChannelId(1));
}
```

- [ ] **Step 2: Run the tests and confirm failure**

Run: `cargo test --lib deleting_selected_object_selects_nearest_and_undo_restores_both -- --exact`

- [ ] **Step 3: Store selection snapshots in history transactions**

Introduce a `WorkbenchSelection` value containing channel, basis, RuleSet, kernel, section, and plot axes. Every draft mutation records before/after selections with the model command, then normalizes surviving IDs.

- [ ] **Step 4: Route mouse and keyboard actions through the same controller methods**

Replace direct mutations in `handle_workbench_panel_mouse` with the public state actions used by `UiCommand`. Decision clicks invoke choice IDs, never screen-specific side effects.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib workbench::state`

Commit: `git add src/workbench/decision.rs src/workbench/state.rs src/app.rs && git commit -m "feat: preserve workbench selection and decisions"`

### Task 3: Correct channel lifecycle and graphical cards

**Files:**
- Modify: `src/workbench/channel_editor.rs`
- Modify: `src/workbench/state.rs:1490-1727`
- Modify: `src/tui/workbench.rs:1011-1067,1359-1709`
- Modify: `src/app.rs:523-598,1058-1065`
- Test: `src/workbench/state.rs`
- Test: `src/tui/workbench.rs`

**Interfaces:**
- Produces: `ChannelCardModel { id, name, color, visible, frozen, selected }`.
- Produces: `WorkbenchState::channel_cards()`, `set_channel_view(ChannelView)`, `toggle_selected_channel_visibility()`, and `toggle_selected_channel_frozen()`.

- [ ] **Step 1: Write failing lifecycle tests**

```rust
#[test]
fn delete_then_add_uses_max_id_and_unfreeze_recreates_every_basis_binding() {
    let mut state = multi_basis_three_channel_state();
    state.select_channel(ChannelId(1)).unwrap();
    state.remove_selected_channel().unwrap();
    state.add_channel().unwrap();
    let ids = state.draft().channels.iter().map(|c| c.id.0).collect::<Vec<_>>();
    assert_eq!(ids, vec![0, 2, 3]);
    state.toggle_selected_channel_frozen().unwrap();
    assert!(state.draft().basis_ids().into_iter().all(|basis|
        state.draft().rules.binding(basis, ChannelId(3)).is_some()));
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --lib delete_then_add_uses_max_id_and_unfreeze_recreates_every_basis_binding -- --exact`

- [ ] **Step 3: Implement atomic lifecycle repair**

Clone the complete draft, compute `max(ChannelId)+1`, apply freeze/unfreeze/remove across every normalized Binding, validate `ExperimentSpec`, then replace the draft in one history transaction. Preserve one-channel minimum and RGB automatic palette.

- [ ] **Step 4: Render and hit-test cards**

Render one card per channel plus the add card. Swatch, eye, freeze, body, and delete each receive a distinct hit region. Add tests that click each subregion and assert only its intended controller action.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib channel`

Commit: `git add src/workbench/channel_editor.rs src/workbench/state.rs src/tui/workbench.rs src/app.rs && git commit -m "feat: make channels a graphical collection"`

### Task 4: Truthful Live/Draft channel previews

**Files:**
- Create: `src/render/workbench_thumbnail.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/app.rs:1841-1950`
- Modify: `src/tui/workbench.rs:1011-1067`
- Test: `src/app.rs`
- Test: `src/tui/workbench.rs`

**Interfaces:**
- Produces: `ChannelPreviewSource::{Live,DraftInitial}`, `ChannelPreviewModel`, and `App::channel_preview_scene(source, view, channel)`.

- [ ] **Step 1: Write a structural-mismatch regression**

```rust
#[test]
fn unapplied_oblique_draft_is_fitted_and_explicitly_not_live() {
    let app = app_with_square_live_and_hex_draft();
    let model = app.channel_preview_model();
    assert_eq!(model.selected_source, ChannelPreviewSource::DraftInitial);
    assert_eq!(model.live_label, "Live · old structure");
    assert_eq!(model.draft_label, "Draft · initial state · not applied");
    assert!(model.apply_and_run_visible);
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --lib unapplied_oblique_draft_is_fitted_and_explicitly_not_live -- --exact`

- [ ] **Step 3: Separate authoritative and draft scene creation**

Do not call `expanded_initial_state` as an implicit fallback. Build Live from the authoritative snapshot/spec pair, DraftInitial from the draft initialization/spec pair, and return `Unavailable(reason)` when either cannot be rendered.

- [ ] **Step 4: Add visual tabs and bounded fitted preview**

Render Composite/Solo/Grid and Live/Draft as clickable tabs. Fit DraftInitial to the preview bounds, keep black board/exterior distinction, and draw a small overlay saying `initial state`.

- [ ] **Step 5: Run phase A gates and commit**

Run: `cargo test --lib && cargo test --test workbench_e2e`

Commit: `git add src/render/workbench_thumbnail.rs src/render/mod.rs src/app.rs src/tui/workbench.rs && git commit -m "feat: distinguish live and draft channel previews"`

---

## Phase B — Multi-kernel collection and editor

### Task 5: Kernel thumbnails, cards, and direct selection

**Files:**
- Modify: `src/render/workbench_thumbnail.rs`
- Modify: `src/workbench/state.rs:1250-1330,1728-1905`
- Modify: `src/tui/workbench.rs:888-946,1104-1307`
- Modify: `src/app.rs:1112-1164,1269-1375`
- Test: `src/tui/workbench.rs`
- Test: `tests/workbench_e2e.rs`

**Interfaces:**
- Produces: `KernelCardModel { id, ordinal, symbol, source, selected, generation }`.
- Produces: `WorkbenchState::kernel_cards()` and `select_kernel(KernelId)`.
- Consumes: `ObjectStripLayout` from Task 1.

- [ ] **Step 1: Write a four-kernel direct-selection test**

```rust
#[test]
fn clicking_each_kernel_card_selects_that_stable_id() {
    let mut app = app_with_four_distinct_kernels();
    for id in [KernelId(3), KernelId(0), KernelId(2), KernelId(1)] {
        let rect = render_and_find_kernel_card(&mut app, id);
        click(&mut app, rect.x + 1, rect.y + 1);
        assert_eq!(app.workbench().selected_kernel(), Some(id));
        assert_eq!(app.workbench().selected_rule_kernel().unwrap().id, id);
    }
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --test workbench_e2e clicking_each_kernel_card_selects_that_stable_id -- --exact`

- [ ] **Step 3: Build cached RGBA thumbnails**

Hash stable kernel ID plus spatial definition and source channel. Cache the compact scene by hash; invalidate only changed kernels. Selection changes reuse cached pixels.

- [ ] **Step 4: Render cards and ordinal**

Show `Kernel 2 of 4`, symbol, source chip, thumbnail, delete, and add. A newly added kernel appears and is selected in the same completed action.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib kernel_card && cargo test --test workbench_e2e clicking_each_kernel_card_selects_that_stable_id -- --exact`

Commit: `git add src/render/workbench_thumbnail.rs src/workbench/state.rs src/tui/workbench.rs src/app.rs tests/workbench_e2e.rs && git commit -m "feat: add visual multi-kernel navigation"`

### Task 6: Predictable kernel deletion and reset

**Files:**
- Create: `src/workbench/growth_symbols.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/workbench/state.rs:1906-1985`
- Modify: `src/sim/growth/mod.rs`
- Test: `src/workbench/growth_symbols.rs`
- Test: `src/workbench/state.rs`

**Interfaces:**
- Produces: `replace_external_symbol(source: &str, symbol: &str, replacement: &str) -> Result<String, Vec<LexError>>`.
- Produces: `request_remove_selected_kernel()`, decision choice `replace-zero-remove`, and `reset_selected_rule_set()`.

- [ ] **Step 1: Write lexical rewrite tests**

```rust
#[test]
fn rewrite_changes_identifier_tokens_but_not_comments_or_longer_names() {
    let source = "let k10 = 2; k1 + k10 // k1";
    assert_eq!(replace_external_symbol(source, "k1", "0").unwrap(),
               "let k10 = 2; 0 + k10 // k1");
}
```

- [ ] **Step 2: Write atomic delete tests**

```rust
#[test]
fn referenced_kernel_delete_is_one_undoable_replace_zero_transaction() {
    let mut state = state_with_growth("k0 + k1");
    state.select_kernel(KernelId(1)).unwrap();
    state.request_remove_selected_kernel().unwrap();
    state.choose_decision("replace-zero-remove").unwrap();
    assert_eq!(state.selected_rule_set_model().kernels.len(), 1);
    assert_eq!(state.growth_editor().buffer().as_str(), "k0 + 0");
    state.undo().unwrap();
    assert_eq!(state.selected_rule_set_model().kernels.len(), 2);
    assert_eq!(state.growth_editor().buffer().as_str(), "k0 + k1");
}
```

- [ ] **Step 3: Verify red**

Run: `cargo test --lib referenced_kernel_delete_is_one_undoable_replace_zero_transaction -- --exact`

- [ ] **Step 4: Implement delete/reset transactions**

Use lexer spans from last to first so byte positions remain valid. Recompile the complete changed source with the reduced ExternalSymbols before committing. On failure keep model/source/selection unchanged. Reset clones the selected shared/default RuleSet after a visible confirmation.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib growth_symbols && cargo test --lib kernel_delete`

Commit: `git add src/workbench/growth_symbols.rs src/workbench/mod.rs src/workbench/state.rs src/sim/growth/mod.rs && git commit -m "feat: make kernel deletion explicit and recoverable"`

### Task 7: Mouse-first kernel palette and complete value editing

**Files:**
- Modify: `src/workbench/kernel_editor.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `src/app.rs`
- Test: `src/workbench/kernel_editor.rs`
- Test: `tests/workbench_e2e.rs`

**Interfaces:**
- Produces: `KernelPaletteModel` with tool, metric, sigma, stencil, anchor, source, output, legend, and reset controls.
- Reuses: `NumericEditor` for wheel and double-click exact entry.

- [ ] **Step 1: Write mouse parity tests**

```rust
#[test]
fn wheel_and_exact_entry_edit_the_same_periodic_cell() {
    let mut app = periodic_hex_kernel_app();
    let point = app.kernel_scene().pixel_for_selection(offset(1, 0), BasisId(0));
    wheel(&mut app, point, 1);
    assert_eq!(app.selected_kernel_value(), 0.05);
    double_click(&mut app, point);
    type_text_and_enter(&mut app, "-0.125");
    assert_eq!(app.selected_kernel_value(), -0.125);
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --test workbench_e2e wheel_and_exact_entry_edit_the_same_periodic_cell -- --exact`

- [ ] **Step 3: Implement graphical palette hit regions**

Weights/Support, Affine/World, Gaussian, sigma, resize, source, output, Fit, and Reset are buttons/chips. Numeric labels open `NumericEditor`; source/output open visible choices.

- [ ] **Step 4: Add persistent legend and empty state**

Render positive cyan, negative red, active zero dark, inactive outline, selected white, anchor gold, and source basis marker. If no kernel exists, show a centered `Add kernel` card rather than black pixels.

- [ ] **Step 5: Run phase B gates and commit**

Run: `cargo test --lib kernel && cargo test --test workbench_e2e kernel`

Commit: `git add src/workbench/kernel_editor.rs src/workbench/state.rs src/tui/workbench.rs src/app.rs tests/workbench_e2e.rs && git commit -m "feat: complete graphical kernel editing"`

---

## Phase C — Growth source and visualization

### Task 8: Referenced-symbol analysis and explicit plot axes

**Files:**
- Modify: `src/workbench/growth_symbols.rs`
- Modify: `src/workbench/growth_editor.rs:18-205`
- Modify: `src/sim/growth/typecheck.rs`
- Test: `src/workbench/growth_editor.rs`

**Interfaces:**
- Produces: `referenced_externals(program: &TypedProgram) -> BTreeSet<String>`.
- Produces: `PlotAxes { x: String, y: Option<String> }`, `set_plot_x(symbol)`, `set_plot_y(Option<symbol>)`, and `pinned_inputs()`.

- [ ] **Step 1: Write axis-policy tests**

```rust
#[test]
fn unused_second_kernel_does_not_force_heatmap() {
    let mut editor = editor_with_inputs("potential", &["potential", "k1"]);
    editor.refresh_now();
    assert_eq!(editor.axes(), &PlotAxes { x: "potential".into(), y: None });
    assert!(editor.plot().curve.is_some());
    editor.replace_source("potential + k1");
    editor.refresh_now();
    assert_eq!(editor.axes().y.as_deref(), Some("k1"));
    assert!(editor.plot().heatmap.is_some());
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --lib unused_second_kernel_does_not_force_heatmap -- --exact`

- [ ] **Step 3: Traverse typed expressions by SymbolId**

Collect only external SymbolIds read by bindings/result expressions, then map them through `program.externals.ordered()`. Ignore local let bindings and names appearing only in comments.

- [ ] **Step 4: Make axes stable by symbol**

On refresh retain valid user axes; otherwise choose self for zero referenced kernel symbols, the sole referenced input for one, and the first two referenced inputs in signature order for two or more. Pin every non-axis scalar deterministically.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib growth_editor`

Commit: `git add src/workbench/growth_symbols.rs src/workbench/growth_editor.rs src/sim/growth/typecheck.rs && git commit -m "feat: derive growth axes from referenced inputs"`

### Task 9: Meaningful high-resolution curve and heatmap

**Files:**
- Modify: `src/workbench/growth_graph.rs`
- Modify: `src/sim/growth/plot.rs`
- Modify: `src/render/workbench_graphics.rs`
- Test: `src/workbench/growth_graph.rs`

**Interfaces:**
- Produces: `GrowthPlotStatus::{Fresh,Stale,NoFiniteSamples(String)}`.
- Produces: curve markers for discontinuities and heatmap zero contour.

- [ ] **Step 1: Write semantic raster tests**

```rust
#[test]
fn equality_function_renders_isolated_sample_marker() {
    let editor = editor_with_source("if potential == 0.5 { 1 } else { 0 }");
    let graph = GrowthGraph::from_editor(&editor);
    let frame = graph.rasterize(900, 360);
    assert!(frame.count_pixels(MARKER_COLOR) > 0);
    assert!(frame.count_pixels(AXIS_COLOR) > 0);
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --lib equality_function_renders_isolated_sample_marker -- --exact`

- [ ] **Step 3: Render axes, domains, status, and finite-sample failures**

Use RGBA text primitives for axis names/ranges, line/marker primitives for curves, and a contour pass for heatmaps. Preserve old data under a translucent STALE overlay. Replace unexplained empty frames with `No finite samples in selected domain`.

- [ ] **Step 4: Test constant, discontinuous, NaN, and two-axis programs**

Assert a constant function has a visible horizontal line, equality has markers, non-finite output has the explicit message, and `potential+k1` changes across both heatmap axes.

- [ ] **Step 5: Run and commit**

Run: `cargo test --lib growth_graph`

Commit: `git add src/workbench/growth_graph.rs src/sim/growth/plot.rs src/render/workbench_graphics.rs && git commit -m "feat: render interpretable growth plots"`

### Task 10: Central source editor, input chips, and concise Inspector

**Files:**
- Modify: `src/tui/workbench.rs:585-780,947-1010,1308-1410,1710-1794`
- Modify: `src/app.rs:483-509,598-640,1066-1111`
- Modify: `src/workbench/state.rs`
- Test: `src/tui/workbench.rs`
- Test: `tests/workbench_e2e.rs`

**Interfaces:**
- Produces: clickable `self`/kernel chips with `AxisRole::{None,X,Y}`.
- Produces: `InspectorTab::{Context,Help}` and scroll state.

- [ ] **Step 1: Write layout tests at three widths**

```rust
#[test]
fn growth_source_keeps_signature_cursor_and_plot_when_inspector_is_narrow() {
    let buffer = render_growth(120, 42, source_with_four_kernels());
    assert!(contains(&buffer, "fn growth("));
    assert!(contains(&buffer, "X potential"));
    assert!(contains(&buffer, "Y k1"));
    assert!(graphics_area(&buffer).height >= 12);
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --lib growth_source_keeps_signature_cursor_and_plot_when_inspector_is_narrow -- --exact`

- [ ] **Step 3: Render central signature/source/chips**

Keep full signature over the source editor; wrap only at argument boundaries. Chips and source occupy Canvas. Cursor, selection, syntax, and span diagnostics use the existing `TextBuffer` state.

- [ ] **Step 4: Make Help opt-in and scrollable**

Default Inspector shows Binding, mode equation, axes, pinned values, validity, and one next action. A Help tab contains syntax/built-ins and consumes wheel only while the pointer is inside Inspector.

- [ ] **Step 5: Add cross-section arity tests**

Add four kernels, delete one, then assert Kernel cards, Growth chips, signature arguments, RuleSet kernels, and `growth.kernel_inputs` all contain exactly three matching IDs/symbols in order.

- [ ] **Step 6: Run phase C gates and commit**

Run: `cargo test --lib growth && cargo test --test workbench_e2e growth`

Commit: `git add src/tui/workbench.rs src/app.rs src/workbench/state.rs tests/workbench_e2e.rs && git commit -m "feat: make growth editing visual and discoverable"`

---

## Phase D — Complete agentic certification and stable release

### Task 11: Replace the incomplete journey with the approved product inventory

**Files:**
- Modify: `tests/agentic/full-journey.md`
- Modify: `docs/testing/agentic-workbench.md`
- Modify: `docs/feature-inventory.md`
- Modify: `tests/workflow_contract.rs`

**Interfaces:**
- Produces: a one-to-one inventory/journey matrix with action, visible result, recovery, persistence, and evidence columns.

- [ ] **Step 1: Write a failing contract test for journey coverage**

```rust
#[test]
fn every_inventory_acceptance_id_has_a_journey_row() {
    let inventory = include_str!("../docs/feature-inventory.md");
    let journey = include_str!("agentic/full-journey.md");
    let ids = inventory
        .split('`')
        .filter(|part| part.starts_with("F-"))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!ids.is_empty(), "inventory has no acceptance IDs");
    for id in ids {
        assert!(journey.contains(&format!("| {id} |")), "missing {id}");
    }
    assert!(!journey.contains("T-junction is permitted"));
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test --test workflow_contract every_inventory_acceptance_id_has_a_journey_row -- --exact`

- [ ] **Step 3: Assign stable acceptance IDs**

Give every user-visible feature in the inventory a `F-<section>-<number>` ID. Rewrite the journey rows so each requires visual discovery, real action, visible outcome, error/cancel path, undo/redo, and Apply/persistence where relevant. Remove the obsolete T-junction acceptance and require strict edge-to-edge rejection.

- [ ] **Step 4: Add mandatory multi-object closed loops**

Require three channels and four kernels; non-sequential selection; distinct edits; middle deletion; cancel/confirm; undo/redo; Growth count/axis changes; save; Apply & Run; leave/re-enter; authoritative verification.

- [ ] **Step 5: Run and commit**

Run: `cargo test --test workflow_contract`

Commit: `git add tests/agentic/full-journey.md docs/testing/agentic-workbench.md docs/feature-inventory.md tests/workflow_contract.rs && git commit -m "test: define complete visual user journeys"`

### Task 12: Strengthen real X11 evidence and cleanup

**Files:**
- Modify: `tests/agentic/scripts/action.sh`
- Modify: `tests/agentic/scripts/capture.sh`
- Modify: `tests/agentic/scripts/session.sh`
- Modify: `tests/agentic_harness.sh`
- Test: `tests/agentic_harness.sh`

**Interfaces:**
- Produces: per-action evidence rows containing sequence, timestamp, full-frame before/after PNG, action, observed result, and Agent verdict.
- Preserves: coordinates are chosen from the latest observed frame, never hard-coded as the product oracle.

- [ ] **Step 1: Add failing lifecycle/evidence shell tests**

Assert captures match the Kitty window pixel dimensions, click coordinates stay inside the observed window, sequence numbers are monotonic, every mutation has before/after files, and stop removes only the recorded process group/X socket/runtime directory.

- [ ] **Step 2: Run and verify red on tinker**

Run: `sh tests/agentic_harness.sh`

- [ ] **Step 3: Implement atomic evidence records**

Write each record only after both PNGs exist and the Agent supplies a non-empty observation/verdict. Record X display, window ID, geometry, PIDs, release identity, and server identity in the manifest.

- [ ] **Step 4: Add ten-minute cadence/resource sampling**

Capture at least once per minute and around suspicious transitions; record process RSS/CPU, Kitty placements, Cellarium shared-memory names, input-to-ack, and input-to-visible. A frozen input or increasing latency fails the run.

- [ ] **Step 5: Run and commit**

Run: `sh tests/agentic_harness.sh`

Commit: `git add tests/agentic/scripts/action.sh tests/agentic/scripts/capture.sh tests/agentic/scripts/session.sh tests/agentic_harness.sh && git commit -m "test: harden real X11 agentic evidence"`

### Task 13: Remote automated gates and exact candidate artifacts

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `docs/releases.md`
- Test: GitHub Actions workflow run

**Interfaces:**
- Produces: checksum-identified x86_64 and ARM64 candidate archives from one commit; the same bytes become stable release assets after certification.

- [ ] **Step 1: Add workflow contract assertions**

Assert release archives contain `cellarium`, include SHA256SUMS, build Linux x86_64 and aarch64, and do not publish until a manually supplied certification artifact names the same commit and checksums.

- [ ] **Step 2: Run remote Rust gates**

Run on tinker:

```bash
cargo fmt --check
cargo test --locked --lib
cargo test --locked --test workbench_e2e
cargo test --locked --test workflow_contract
cargo test --locked --test remote_e2e
cargo check --locked --target aarch64-unknown-linux-gnu
```

Expected: every command exits 0; no local Raspberry Pi build occurs.

- [ ] **Step 3: Push the implementation commit and build a draft candidate**

Trigger the release workflow in draft mode for the exact commit. Download x86_64 to tinker and ARM64 to the Pi through the release asset URLs, verify SHA256SUMS, and record binary `--version` plus executable hash.

- [ ] **Step 4: Commit workflow changes**

Commit: `git add .github/workflows/ci.yml .github/workflows/release.yml docs/releases.md && git commit -m "ci: gate stable releases on certified artifacts"`

### Task 14: Execute the complete adaptive agentic journey

**Files:**
- Evidence only: test-owned directory outside the repository
- Update after pass: `docs/testing/agentic-workbench.md`

**Interfaces:**
- Consumes: exact candidate artifacts from Task 13.
- Produces: complete evidence ledger and stable-release certification.

- [ ] **Step 1: Install exact binaries without compiling locally**

Install the verified x86_64 asset as `/home/wkj/.local/bin/cellarium` on tinker. Extract the verified ARM64 asset into the isolated Pi test directory.

- [ ] **Step 2: Start the 1:1 environment**

Start isolated Xvfb, Openbox, Kitty, and `cellarium connect tinker`. Record all PIDs/window IDs and confirm one client plus one test server before interaction.

- [ ] **Step 3: Perform every inventory journey as a user**

For every row: inspect the full current screenshot, locate the visible control, send one real X11 mouse/keyboard action, inspect the next screenshot, and record the semantic visual judgment. Do not use source, trace, hash, or fixed coordinates as the pass oracle.

- [ ] **Step 4: Complete the required design workflow**

Starting from blank: create a strict edge-to-edge triangular tiling, add RGB channels, add four kernels, switch them non-sequentially, paint distinct values/support, exact-edit a float, delete the middle kernel, undo/redo, author a valid Growth program, change axes, save, Apply & Run, observe non-square live geometry, return and reload.

- [ ] **Step 5: Exercise errors and recovery**

Attempt duplicate polygon point, non-closed/crossing/gapped tiling, invalid numeric input, referenced-kernel deletion cancel/replace, invalid Growth syntax, stale plot repair, invalid Apply, disconnect/reconnect, and half-block fallback.

- [ ] **Step 6: Run sustained mixed use**

For at least ten minutes combine resize, pan/zoom, section transitions, add/delete, undo/redo, editing, Apply, leave/re-enter, and reconnect. Fail immediately on stale placement, blank unexplained plot, selection drift, frozen input, duplicate ID, or unrecoverable state.

- [ ] **Step 7: Clean up and prove cleanup**

Stop recorded groups only. Verify no test X socket, Xvfb/Openbox/Kitty/client/server child, Kitty image placement, or Cellarium shared-memory object remains.

- [ ] **Step 8: Publish a real stable release**

Only with zero unresolved defects, publish the already-certified draft as a normal stable GitHub Release without changing assets. Re-download public assets and re-verify checksums. If any row fails, preserve evidence, fix with a regression test, rebuild a new candidate, and restart the affected journey plus sustained test.

- [ ] **Step 9: Record final certification commit**

Commit the exact tag, commit, asset names, hashes, environment, result table, and evidence paths:

```bash
git add docs/testing/agentic-workbench.md
git commit -m "docs: certify complete visual workbench journey"
```

## Final verification

Run on tinker:

```bash
git diff --check
cargo fmt --check
cargo test --locked --lib
cargo test --locked --test workbench_e2e
cargo test --locked --test workflow_contract
cargo test --locked --test remote_e2e
cargo check --locked --target aarch64-unknown-linux-gnu
```

Then confirm the stable public ARM64 and x86_64 assets have the exact certified SHA256 values and the test-owned processes/resources are gone.
