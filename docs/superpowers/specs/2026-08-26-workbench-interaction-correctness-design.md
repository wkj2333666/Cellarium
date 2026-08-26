# Workbench Interaction Correctness Design

**Date:** 2026-08-26  
**Status:** Approved in chat; implementation planning pending document review

## Objective

Make the released Cellarium client visually truthful and directly operable throughout the
Simulation and Workbench journeys. A user must never see pixels from an obsolete editor
state, must be able to select every rendered object that the current tool permits, and must
receive actionable feedback when an operation is unavailable.

The result is accepted only after automated regression tests pass on tinker and the exact
released ARM64 client is exercised through real Kitty/X11 mouse and keyboard input on the
local Raspberry Pi.

## Hard constraints

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

## Observed failures

The 2026-08-26 local agentic journey used the v0.2.0 ARM64 release client in a real
Xvfb → Openbox → Kitty stack and connected to the tinker GPU server.

1. A new 256×256 simulation starts at zoom 1.0 and occupies only about 256 pixels in a
   substantially larger viewport.
2. Changing a one-basis square draft to a one-basis hexagonal draft before Apply makes
   Channels render authoritative square-state values through the uncommitted hex geometry.
3. A zero-kernel or all-zero kernel page is an unexplained black canvas.
4. Exact-equality growth programs such as `potential == 2/6` appear as a flat zero curve
   because uniform sampling misses isolated values.
5. Pressing `B` in Tiling changes the model to “No periodic tiling yet” but leaves the
   previous Kitty placement visible.
6. Undoing the third construction vertex changes the model to two vertices but leaves the
   pointer at the removed point, so the live preview still looks like the old triangle.
7. In Weights mode, inactive periodic kernel cells are filtered out during hit-testing.
   They look selectable but clicks produce no state or explanation.
8. Channel rows in the Inspector look like list items but mouse clicks only focus the panel.
9. Long one-line toolbars clip actions and make clipped mouse actions unreachable.
10. Removing a kernel removes its generated growth argument without checking whether the
    source still references that symbol; Apply later fails with `unknown_symbol`.
11. At low initial zoom, a correctly polygon-rasterized custom topology is visually
    indistinguishable from a rectangular pixel lattice.

## Design

### 1. Explicit Workbench graphics presence

Introduce an explicit scene-presence decision before presentation:

- **Pixels:** the section has a current raster scene and supplies a generation.
- **Empty:** the section intentionally has no raster scene.
- **Text:** the section is rendered with ordinary terminal widgets.

The decision is made from the current Workbench state before
`prepare_workbench_scene`. A transition to Empty or Text invalidates pending asynchronous
work, deletes the current Kitty placement, resets the surface generation, and forces covered
terminal cells to be emitted. A transition to Pixels preserves the old image only while a
real replacement is pending.

Tiling with neither a draft nor construction is Empty. Kernels with no selected definition
is Empty. Experiment is Text. World, Growth, and Channels are Pixels. Tiling construction
and valid Tiling drafts are Pixels.

This replaces call-site guesses with one lifecycle invariant:

> If the model says no graphic exists, no graphics placement may remain on screen.

The same transition contract is used for Kitty, inline pixel protocols, and half-block. The
half-block path clears its character-cell area when presence becomes Empty or Text.

### 2. Construction preview truthfulness

Construction pointer state is part of the displayed model. Removing the final construction
vertex sets the pointer to the new final vertex or clears it. Cancelling, starting a blank
design, finishing a polygon, changing tools, and changing section clear obsolete pointer
state.

Undo/redo regression tests assert both vertex count and rendered geometry. A two-vertex
construction may show only the segment between those vertices plus a live segment to the
current physical pointer; it must not retain a removed point.

### 3. Draft and authoritative preview separation

Runtime compatibility becomes structural instead of cardinality-only. It compares:

- raster dimensions;
- complete ordered basis IDs;
- channel IDs and count;
- the active and draft periodic tiling, including translations, prototypes, instances, and
  transforms.

Channels uses authoritative runtime values only when that structure is identical. Otherwise
it renders the draft initial field and labels the canvas `Preview: draft initial state`.
No running snapshot is ever interpreted through unrelated draft geometry.

Kernel/growth metadata remains draft-owned inside Workbench. Simulation remains
authoritative-active until Apply succeeds.

### 4. Geometry-aware camera fitting

Add a pure camera-fit calculation shared by rectangular and basis scenes. It computes the
world-space bounds of all cells in the finite domain, preserves aspect ratio, centers the
domain, and leaves a small visual margin.

Automatic fit runs:

- after the first non-zero simulation viewport is known;
- after a successful Apply changes geometry or dimensions;
- after loading a persisted experiment into a viewport for the first time.

It does not repeatedly override user pan or zoom. A resize refits only until the user has
manually changed the camera; `0` remains an explicit fit action afterward.

For polygonal topology the bound calculation uses translation vectors and transformed
prototype polygons, not raster width/height alone. This makes visible hexagonal geometry
legible without changing simulation values or adding permanent cell outlines.

### 5. Periodic kernel selection and empty states

Periodic kernel rendering and hit-testing continue using one
`PeriodicPixelTransform`. Selection behavior is split from edit permission:

- Inspect selects any rendered active or inactive cell.
- Support mode can activate or deactivate support cells.
- Weights mode can edit only active cells.
- Attempting to edit an inactive cell keeps it selected and displays an actionable message
  telling the user to switch to Support mode.

Mouse wheel and exact-value editing use the selected cell returned by this unfiltered
inspection. Tests cover center, every visible edge cell after terminal-cell quantization,
inactive cells, zoom, and pan.

If no kernel exists, the canvas is explicitly cleared and shows an ordinary empty-state
message with `A Add kernel`. An all-zero kernel still renders outlines, active/inactive
marks, the anchor, and a zero-valued legend instead of an undifferentiated black rectangle.

### 6. Dependency-safe kernel removal

Removing the final kernel remains forbidden. Before removing any other kernel:

1. identify its generated input symbol;
2. typecheck the existing growth source against the proposed post-removal signature;
3. reject removal if the source still references that symbol;
4. show which symbol must be removed or replaced in Growth.

A successful removal updates kernel inputs, refreshes the Growth editor signature and plot,
and preserves a valid draft at every history entry. Undo restores the kernel and its
signature atomically. There is no intermediate draft that can produce `unknown_symbol`
only at Apply time.

### 7. Discontinuity-aware growth plots

Keep the existing bounded uniform sampling and add critical probes derived from the typed
program. Traverse comparisons involving the selected axis and a constant-foldable scalar
expression. For each in-domain threshold, evaluate immediately below, exactly at, and
immediately above the value.

Curve data retains each sample's real input coordinate. Rendering places samples by input
rather than vector index, draws continuous segments where appropriate, and draws isolated
point markers for equality-only outputs. Thus `potential == 2/6` and
`potential == 3/6` become visible without altering expression semantics.

The graph also exposes a short plot diagnostic when it is constant, entirely invalid, or
contains isolated discontinuities. Plot probes remain editor-only and do not affect runtime
evaluation.

### 8. Responsive, clickable Workbench controls

Compute toolbar layout once from actual canvas width. Segments wrap across as many header
rows as required within a small bounded header height. Rendering and mouse hit-testing use
the same row/column segment rectangles. No visually clipped action retains an invisible
hit target.

The Channels Inspector exposes row rectangles. Clicking a row selects the channel and
updates the canvas, signature target, and selection marker. Clicking outside a row only
changes focus. Growth help gets a visible scroll-position indicator when content exceeds
the panel.

Dense layouts retain keyboard shortcuts and move secondary descriptions to the Inspector
rather than silently truncating them.

## Error handling and recovery

- Empty graphics transitions are idempotent.
- Obsolete asynchronous frames are invalidated before their placement can be presented.
- A failed new frame leaves either the previous valid Pixels scene or a truthful Empty/Text
  scene; it never combines old pixels with new labels.
- Invalid persisted zero-kernel drafts load into an explicit recoverable empty state and can
  add a kernel, while Apply remains structurally validated.
- Rejected kernel deletion does not enter history or dirty the draft.
- Camera fit rejects non-finite or degenerate bounds and falls back to the current camera
  with a visible notice.

## Verification strategy

### Automated RED→GREEN tests on tinker

Each defect receives a behavior test that is observed failing before production changes:

- graphics presence transitions: Pixels→Empty, Pixels→Text, and stale async completion;
- blank Tiling and zero-kernel deletion commands;
- construction undo clears the removed pointer and changes frame pixels;
- exact active/draft topology compatibility;
- rectangular and oblique finite-domain camera fit;
- inactive periodic kernel inspection and active-only editing;
- complete terminal-cell reachability for periodic kernel polygons;
- dependency-safe kernel deletion and undo;
- discontinuity probe extraction and non-uniform x-coordinate rendering;
- wrapped toolbar rendering/hit-testing equivalence;
- clickable channel rows at narrow and wide layouts.

Run focused tests after every RED→GREEN change, then the complete locked test suite,
formatting, lint, and release build on tinker.

### Protocol and PTY tests

The client/server E2E journey verifies Apply, authoritative revision, snapshots, input
acknowledgement, editor transitions, and half-block interaction. Waits are
condition-based—frame generations, trace events, or semantic screen state—not arbitrary
sleep-based success checks.

### Real agentic release validation

After CI publishes a stable release, download its ARM64 asset locally. In an isolated data
directory, run the real journey through Xvfb → Openbox → Kitty:

1. open the simulation and verify automatic fit;
2. enter Workbench;
3. create a blank Tiling and confirm old graphics disappears;
4. draw three triangle vertices, undo one, and confirm both model and pixels show two;
5. close, choose hex preset, inspect neighboring cells, Apply & Run, and verify polygonal
   domain geometry;
6. change an unapplied topology and confirm Channels says draft initial state;
7. add three channels, click each row, verify RGB defaults and selection;
8. inspect inactive and active kernel cells, activate support, wheel-adjust, and enter an
   exact value;
9. attempt unsafe kernel deletion and verify immediate rejection;
10. enter the equality-based Growth program and verify visible isolated plot markers;
11. exercise toolbar clicks at the supported minimum and wide terminal sizes;
12. leave/re-enter Workbench and switch every section while checking placement cleanup;
13. repeat the interaction-critical journey in half-block mode.

Every action is followed by observation of the real framebuffer. An accepted xdotool event
is not considered success. Any new user-visible defect found by this journey re-enters the
RED→GREEN loop before release acceptance.

## Acceptance criteria

- All eleven observed failures have a passing automated regression.
- The complete tinker suite, format check, lint, and release build exit successfully.
- The released ARM64 artifact completes the agentic journey without stale graphics,
  unselectable visible objects, unexplained empty canvases, clipped essential actions, or
  active/draft misrepresentation.
- No local Raspberry Pi build or performance benchmark is run.
- No unreconciled Cellarium, Kitty, Openbox, or Xvfb process remains after testing.
- Release is stable and its artifact SHA-256 is recorded in the final evidence.

