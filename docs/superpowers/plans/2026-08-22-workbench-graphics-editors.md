# Workbench Graphics Editors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver interactive Kitty-graphics and half-block Workbench editors for tilings, kernels, and growth programs, with reliable SSH mouse gestures and user-level C/S verification.

**Architecture:** Keep Ratatui for chrome, navigation, Inspector, and text editing. Add a shared CPU RGBA preview surface whose Kitty and half-block presenters consume the same frame. Route every mouse Down/Drag/Up through the existing input protocol and apply drafts only after server acknowledgement.

**Tech Stack:** Rust, crossterm, Ratatui, existing `image` and Kitty shared-memory display code, serde remote protocol, PTY/tmux probes on `tinker`, GitHub Release precompiled binaries.

**Spec:** `docs/superpowers/specs/2026-08-22-workbench-graphics-editors-design.md`

## Global Constraints

- Never run Cargo build, test, benchmark, or performance measurement on the local machine.
- All build/test commands run on `tinker` through the SSH bridge.
- C/S tests download a pinned GitHub Release asset, verify `SHA256SUMS`, record tag/version/hash, then run that binary; no locally compiled client or server is accepted.
- Kitty and half-block presenters consume the same logical RGBA frame and the same input hit-testing model.
- Preview work is bounded and dirty-generation driven; it must not run at simulation frame rate.
- Existing simulation direct-render mode and non-Workbench behavior remain functional.

---

### Task 1: Add the shared Workbench graphics surface

**Files:**
- Create: `src/render/workbench_graphics.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/render/display/mod.rs`
- Test: `src/render/workbench_graphics.rs` (unit tests)

**Interfaces:**
- Produce `pub struct GraphicsFrame { pub width: u32, pub height: u32, pub rgba: Vec<u8>, pub generation: u64 }`.
- Produce `pub trait GraphicsScene { fn render_rgba(&self, width: u32, height: u32) -> GraphicsFrame; }`.
- Produce `pub struct GraphicsSurface { pub fn mark_dirty(&mut self); pub fn present(&mut self, frame: GraphicsFrame) -> PresentResult; }`.
- Consume the existing `DisplayProtocol`, Kitty shared-memory placement lifecycle, and half-block renderer.

- [ ] **Step 1: Write failing frame and bounds tests.** Verify RGBA length is exactly `width * height * 4`, zero-sized frames are rejected, generations replace older pending frames, and a frame is marked fresh only once.
- [ ] **Step 2: Run the focused remote test.** Run `CARGO_TARGET_DIR=/tmp/cellarium-plan-graphics cargo test --locked --no-default-features render::workbench_graphics` in `/home/wkj/projects/cellarium` on `tinker`; expect the new tests to fail before implementation.
- [ ] **Step 3: Implement the frame/surface types.** Keep the scene renderer independent of terminal output and cap image dimensions to the terminal viewport.
- [ ] **Step 4: Add Kitty presentation integration.** Derive deletion from the placement displayed at presentation time, retain shared-memory objects until terminal consumption/timeout, and fall back to half-block on allocation or protocol failure.
- [ ] **Step 5: Run the focused remote test again.** Expect all frame lifecycle tests to pass.
- [ ] **Step 6: Commit.** `git add src/render/workbench_graphics.rs src/render/mod.rs src/render/display/mod.rs && git commit -m "feat: add shared workbench graphics surface"`.

### Task 2: Repair mouse gesture forwarding and coordinate transforms

**Files:**
- Modify: `src/input.rs`
- Modify: `src/app.rs`
- Modify: `src/remote.rs`
- Test: `src/input.rs`, `src/app.rs`, `src/remote.rs`

**Interfaces:**
- Change `MouseTracker::update` to return a gesture event that distinguishes `Down`, `Action`, and `Up`, while preserving `MouseAction` for camera/draft operations.
- Add `pub fn map_viewport_point(event: MouseEvent, viewport: Rect, logical_size: [u32; 2]) -> LogicalPoint` in `src/input.rs` and use it for Workbench and simulation paths.
- Keep `InputMessage::Mouse(MouseEvent)` wire compatibility and ensure middle Down/Up are transmitted.

- [ ] **Step 1: Add failing tests.** Assert that middle Down is forwarded, the first middle Drag uses the Down coordinate, Up clears state, focus loss clears state, and a terminal point maps to the same logical cell used by rendering.
- [ ] **Step 2: Run remote focused tests and record RED.** Use the same `/tmp/cellarium-plan-graphics` target directory on `tinker`.
- [ ] **Step 3: Implement complete gesture forwarding.** The client sends Down/Drag/Up even when no immediate visual action occurs; the server tracker receives the same sequence.
- [ ] **Step 4: Replace duplicated coordinate arithmetic.** Route Workbench world, tiling, kernel, and simulation camera mapping through `map_viewport_point`, including pixel-center handling and clamping.
- [ ] **Step 5: Run focused tests and verify GREEN.** Include a C/S protocol round-trip test for middle-button events.
- [ ] **Step 6: Commit.** `git add src/input.rs src/app.rs src/remote.rs && git commit -m "fix: forward complete mouse gestures over remote input"`.

### Task 3: Build the Tiling graphics scene and mouse editor

**Files:**
- Create: `src/workbench/tiling_editor.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/tiling_editor.rs`, `tests/workbench_graphics.rs`

**Interfaces:**
- Add `pub struct TilingScene` implementing `GraphicsScene` from a `PeriodicTilingDraft`, selected `PrototypeId`, camera, and validation report.
- Add `pub enum TilingGesture { Select(Selection), AddVertex(Vec2), MoveVertex { index: usize, to: Vec2 }, RemoveVertex(usize), FinishPolygon }`.
- Add `pub fn handle_tiling_mouse(&mut self, event: MouseEvent) -> Result<bool, GeometryIssue>` on `WorkbenchState`.

- [ ] **Step 1: Write failing model tests.** Cover lattice-vector rendering coordinates, arbitrary simple polygons, vertex hit testing, add/move/remove gestures, and invalid self-intersection diagnostics.
- [ ] **Step 2: Run remote focused tests and record RED.** Use `cargo test --locked --no-default-features workbench::tiling_editor tests/workbench_graphics` on `tinker`.
- [ ] **Step 3: Implement scene rasterization.** Draw the fundamental domain, neighboring translated copies, prototype fills/outlines, vertex handles, selected edges, and invalid geometry overlays into RGBA.
- [ ] **Step 4: Implement gesture state.** Add Select/Draw/Vertex modes, double-click close, secondary-button delete in vertex mode, snap-to-grid toggle, and undoable draft commands.
- [ ] **Step 5: Integrate Canvas focus and Inspector fields.** Show the active mode, selected prototype, vertex index/coordinates, translation vectors, and validation messages.
- [ ] **Step 6: Run focused tests and commit.** `git add src/workbench/tiling_editor.rs src/workbench/mod.rs src/workbench/state.rs src/tui/workbench.rs tests/workbench_graphics.rs && git commit -m "feat: add interactive graphics tiling editor"`.

### Task 4: Build the Kernel heatmap editor

**Files:**
- Create: `src/workbench/kernel_editor.rs`
- Modify: `src/workbench/mod.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/kernel_editor.rs`, `tests/workbench_graphics.rs`

**Interfaces:**
- Add `pub struct KernelScene` implementing `GraphicsScene` from a `KernelDefinition`, selected cell, zoom, and camera.
- Add `pub enum KernelGesture { Select { x: usize, y: usize }, Paint { x: usize, y: usize, value: f32 }, Clear { x: usize, y: usize }, Pan { dx: f32, dy: f32 } }`.
- Add `pub fn handle_kernel_mouse(&mut self, event: MouseEvent) -> Result<bool, KernelError>` and typed Inspector field operations for dimensions, anchor, normalization, and selected value.

- [ ] **Step 1: Write failing tests.** Check heatmap color mapping for positive/negative/zero values, mask rendering, anchor coordinates, cell hit testing under zoom, resize clamping, and growth-input arity after kernel add/remove.
- [ ] **Step 2: Run remote focused tests and record RED.** Use the remote no-default Cargo test target.
- [ ] **Step 3: Implement matrix rasterization.** Draw cells with a legend and numeric range; distinguish disabled mask cells from enabled zero cells; draw anchor crosshair and selected-cell outline.
- [ ] **Step 4: Implement mouse and Inspector editing.** Paint/clear cells, pan/zoom, edit typed fields, clamp dimensions to kernel limits, resize values/mask deterministically, and add one undo command per completed gesture.
- [ ] **Step 5: Integrate arity validation.** Keep every growth source's kernel input count consistent and expose an actionable diagnostic when the draft is incomplete.
- [ ] **Step 6: Run focused tests and commit.** `git add src/workbench/kernel_editor.rs src/workbench/mod.rs src/workbench/state.rs src/tui/workbench.rs tests/workbench_graphics.rs && git commit -m "feat: add interactive graphics kernel editor"`.

### Task 5: Upgrade the Growth text editor

**Files:**
- Modify: `src/workbench/text_buffer.rs`
- Modify: `src/workbench/growth_editor.rs`
- Modify: `src/app.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/text_buffer.rs`, `src/workbench/growth_editor.rs`, `tests/workbench_graphics.rs`

**Interfaces:**
- Extend `TextBuffer` with `selection()`, `set_selection()`, `line_column()`, `move_word_left()`, `move_word_right()`, and `replace_selection()`.
- Add `pub struct GrowthEditorView { pub cursor: TextPosition, pub selection: Option<TextRange>, pub scroll: ScrollOffset }`.
- Add `pub fn handle_growth_editor_key(&mut self, key: KeyEvent) -> bool` and `pub fn handle_growth_editor_mouse(&mut self, event: MouseEvent) -> bool`.

- [ ] **Step 1: Write failing cursor/selection tests.** Cover UTF-8 boundaries, shift-selection, word movement, deletion, vertical scrolling, and diagnostic source-span highlighting.
- [ ] **Step 2: Run remote focused tests and record RED.** Use the remote Cargo target.
- [ ] **Step 3: Implement text-buffer selection and view state.** Preserve existing editing semantics and make Escape leave edit mode without quitting the application.
- [ ] **Step 4: Implement the Ratatui editor widget.** Draw line numbers, token styles, current-line highlight, visible cursor, selection, diagnostics, and scroll indicators; remove the ASCII plot from the editor text.
- [ ] **Step 5: Debounce compile refresh.** Keep the last valid plot when input is invalid and expose diagnostics in both editor and Inspector.
- [ ] **Step 6: Run focused tests and commit.** `git add src/workbench/text_buffer.rs src/workbench/growth_editor.rs src/app.rs src/tui/workbench.rs tests/workbench_graphics.rs && git commit -m "feat: add usable growth source editor"`.

### Task 6: Rasterize and present the Growth graph

**Files:**
- Create: `src/workbench/growth_plot.rs`
- Modify: `src/workbench/growth_editor.rs`
- Modify: `src/tui/workbench.rs`
- Test: `src/workbench/growth_plot.rs`, `tests/workbench_graphics.rs`

**Interfaces:**
- Add `pub struct GrowthPlotScene` implementing `GraphicsScene` from `GrowthPlot`, channel colors, selected input, and plot bounds.
- Add `pub fn rasterize_growth_plot(plot: &GrowthPlot, style: PlotStyle, size: (u32, u32)) -> GraphicsFrame`.

- [ ] **Step 1: Write failing pixel tests.** Verify axes/grid occupy expected bounds, curve pixels change when samples change, invalid samples produce markers, and channel colors appear in the legend.
- [ ] **Step 2: Run remote focused tests and record RED.** Use the remote Cargo test target.
- [ ] **Step 3: Implement the RGBA plot renderer.** Draw padded axes, ticks, grid, zero line, curve(s), selected sample crosshair, stale/invalid overlay, and legend without adding a plotting dependency.
- [ ] **Step 4: Mount the scene through `GraphicsSurface`.** Kitty receives the direct image; half-block receives the same pixels. Preview refresh is debounce-limited and independent of simulation rate.
- [ ] **Step 5: Run focused tests and commit.** `git add src/workbench/growth_plot.rs src/workbench/growth_editor.rs src/tui/workbench.rs tests/workbench_graphics.rs && git commit -m "feat: render precise graphics growth plots"`.

### Task 7: Compose Workbench surfaces and make every section discoverable

**Files:**
- Modify: `src/tui/workbench.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/app.rs`
- Modify: `src/tui/mod.rs`
- Test: `tests/workbench_e2e.rs`, `tests/pty_startup.rs`

**Interfaces:**
- Add `pub fn draw_workbench_graphics(frame: &mut ratatui::Frame, app: &mut App, area: Rect)` as the single Workbench entry point.
- Add a `WorkbenchSurfaceKind` selector mapping each section/focus to a scene and editor view.
- Keep `App::set_viewport` and `App::viewport_geometry` backed by the active Canvas inner rectangle only.

- [ ] **Step 1: Add failing PTY navigation tests.** Click every left section, verify the Canvas title/scene changes, enter each editor, and verify the Inspector exposes the current controls.
- [ ] **Step 2: Run remote PTY tests and record RED.** Use the existing PTY harness on `tinker`.
- [ ] **Step 3: Replace text-only non-World Canvas branches.** Mount Tiling, Kernel, and Growth graphics surfaces while keeping Ratatui chrome and Inspector.
- [ ] **Step 4: Wire focus and mouse hit testing.** Canvas gestures go to the active editor; Inspector clicks select fields; section clicks never get consumed by the Canvas.
- [ ] **Step 5: Fix clipping and cleanup.** Ensure footer/Inspector text stays within bounds, switching sections deletes/replaces the prior Kitty placement, and leaving Workbench clears all previews.
- [ ] **Step 6: Run PTY tests and commit.** `git add src/tui/workbench.rs src/workbench/state.rs src/app.rs src/tui/mod.rs tests/workbench_e2e.rs tests/pty_startup.rs && git commit -m "feat: compose interactive workbench surfaces"`.

### Task 8: Add release-based C/S user-level test harness

**Files:**
- Create: `tests/support/release_artifact.rs`
- Create: `tests/remote_workbench_user.rs`
- Modify: `tests/support/terminal_probe.rs`
- Modify: `docs/e2e/README.md`

**Interfaces:**
- Add `ReleaseArtifact::download_and_verify(tag, target) -> Result<ReleaseArtifact>` that downloads the release asset and `SHA256SUMS`, verifies the hash, and returns immutable binary paths plus version/hash metadata.
- Add `TerminalProbe::send_sgr_mouse`, `TerminalProbe::send_key`, `TerminalProbe::wait_for_ack`, `TerminalProbe::kitty_frames`, and `TerminalProbe::halfblock_image`.
- Add `UserScenarioReport` fields for server sim Hz, snapshot rx Hz, UI draw Hz, fresh preview Hz, ack sequence, stale placements, and process responsiveness.

- [ ] **Step 1: Write failing release and ack-gating tests.** Verify missing/incorrect checksums fail, optimistic local state does not count as success, and a middle Down/Drag/Up scenario waits for server acknowledgement.
- [ ] **Step 2: Run the non-network harness tests remotely and record RED.** The command must use the `--no-default-features` test configuration and `/tmp/cellarium-plan-graphics` target directory.
- [ ] **Step 3: Implement release acquisition.** Pin a release tag supplied by the test command, use the architecture asset matching tinker, verify `SHA256SUMS`, and record the exact binary version/hash.
- [ ] **Step 4: Implement actual PTY C/S launch.** Start the downloaded server binary on tinker, launch the downloaded client binary through the real connector path, and capture both Kitty APC and half-block output.
- [ ] **Step 5: Implement user scenarios.** Cover Workbench navigation, polygon draw/drag/delete, kernel paint/resize/value edit, growth cursor/type/diagnostic/plot update, Apply/Revert, middle pan, rapid gestures, resize, fallback, reconnect, and clean exit.
- [ ] **Step 6: Run the harness against the pinned release and commit.** `git add tests/support/release_artifact.rs tests/remote_workbench_user.rs tests/support/terminal_probe.rs docs/e2e/README.md && git commit -m "test: add release-based workbench user scenarios"`.

### Task 9: Remote acceptance runs and release gate

**Files:**
- Modify: `docs/e2e/README.md`
- Create: `target/e2e/workbench-graphics-user.json` (generated report, not hand-edited)

- [ ] **Step 1: Download and verify the candidate GitHub Release on tinker.** Record tag, asset URL, version, SHA-256, terminal type, and server/client command lines.
- [ ] **Step 2: Run Kitty acceptance.** Use `TERM=xterm-kitty` with graphics enabled; execute every scenario from Task 8, require all input acks, inspect Kitty pixels/placements, and assert no stale image remains after section changes or fallback.
- [ ] **Step 3: Run half-block acceptance.** Use a non-Kitty TERM; execute the same scenarios, verify the reconstructed raster probes and middle pan, and assert no interaction path is disabled.
- [ ] **Step 4: Run stress and recovery.** Perform 100 alternating paint/erase/drag gestures, 20 section switches, three resize events, disconnect/reconnect, and clean quit; require bounded latency and no unresponsive process.
- [ ] **Step 5: Publish the JSON report.** Include separate server/snapshot/UI/fresh-preview rates and failed scenario details. Do not label UI draw rate as graphics consume rate.
- [ ] **Step 6: Run remote verification before claiming completion.** Execute the focused tests plus the release-based scenarios on tinker; inspect exit codes, process-continuation flags, report contents, and `git diff --check`.
- [ ] **Step 7: Commit the report and implementation release gate.** `git add docs/e2e/README.md target/e2e/workbench-graphics-user.json && git commit -m "test: record workbench graphics acceptance"`.

## Execution order

Run Tasks 1 and 2 first because every editor depends on the shared surface and complete gesture sequence. Then run Tasks 3–6 in order; Task 7 composes them. Task 8 is written alongside Task 7 but cannot run until the protocol and surface changes are present. Task 9 is the final remote-only gate and must use a GitHub Release binary, never a local build.
