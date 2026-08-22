# Workbench Graphics Editors Design

**Status:** Design approved in conversation; implementation has not started.

## Goal

Make the Workbench a usable editor for initial cell fields, polygon tilings, kernels, channels, and growth programs. Kitty terminals must receive precise direct-rendered previews. Terminals without Kitty graphics must render the same preview scene through a functional half-block fallback. Local and C/S input semantics must be identical, including middle-button panning.

## User-visible requirements

1. The Workbench has three visible regions: section navigation, a central Canvas, and an Inspector/editor panel. Clicking a section changes the central Canvas and Inspector content. The Canvas is an editor, not a text explanation.
2. Tiling Canvas shows lattice vectors, periodic boundaries, prototype polygons, and repeated instances. Users can select a prototype, draw a polygon with the mouse, drag vertices, add/remove vertices, and see invalid geometry highlighted immediately.
3. Kernel Canvas shows the selected kernel's matrix as a precise heatmap with mask and anchor overlays. Users can select cells, paint values by dragging, clear cells with the secondary button, pan/zoom, change dimensions and anchor, and edit numeric values through a visible Inspector field.
4. Growth editing provides a real text editor with a visible cursor, current-line highlight, line numbers, horizontal/vertical scrolling, syntax highlighting for the supported expression language, and diagnostics tied to source spans. The plot is a precise graphical curve with axes, grid, legend, invalid-region indication, and a selected sample marker; it is never an ASCII sparkline.
5. Channel visualization uses the configured channel colors and visibility. The default one-channel display remains high-contrast; three-channel defaults are red/green/blue; the domain background remains the existing dark background.
6. The existing simulation direct-render mode remains available. Entering and leaving Workbench must not leak Kitty images or paint a previous Canvas over a new one.
7. C/S clients use the server as the authority after Apply. A local draft may be edited optimistically, but success indicators and E2E assertions require the corresponding server sequence/ack and authoritative snapshot.

## Compatibility and hard constraints

- Do not run a local Cargo build, test, benchmark, or performance measurement for this feature.
- Every C/S build or test must run on `tinker` and must use precompiled binaries downloaded from a GitHub Release. The test harness must record the release tag, asset URL, SHA-256, and binary version before running.
- The test matrix must include Kitty graphics and a non-Kitty half-block terminal. Both modes must support navigation, editing, apply/revert, and mouse gestures.
- Preview rendering must be bounded and event-driven. It must not run at simulation frame rate, allocate unbounded terminal image IDs, or make ARM client performance claims.
- Existing raster simulation execution remains the compatibility path even when polygon tiling metadata is being edited. Apply rejects invalid drafts before replacing the running specification.

## Recommended architecture

### 1. Shared graphics surface

Extract a reusable Workbench preview surface beside the existing simulation display code. A surface owns a bounded RGBA image, a logical scene size, a placement identity, and a dirty generation. The scene renderer is independent of the terminal protocol. Kitty uses the existing shared-memory/placement lifecycle, with deletion derived from the currently presented placement. Non-Kitty terminals use the same RGBA pixels converted to half-block cells; no separate editor behavior is permitted.

Preview generations are published only when the draft, selection, camera, or editor cursor changes, with a small debounce for text input and a maximum image size suitable for the terminal viewport. A pending generation replaces older pending work. The surface reports `fresh_frame` separately from UI draw rate.

### 2. Workbench state and focus

Keep the existing `WorkbenchState` as the authoritative draft/undo model. Add explicit Canvas modes (`Select`, `DrawPolygon`, `PaintKernel`, `Pan`) and a focus model that distinguishes section list, Canvas, Inspector fields, and Growth text editor. Every visible shortcut is derived from the current focus and rendered in the Inspector footer. Mouse hit-testing maps terminal coordinates to the active scene's logical coordinates through one shared transform.

### 3. Tiling editor

Render the periodic cell using `PeriodicTilingDraft`: translation A/B define the fundamental domain; prototypes render as transformed polygons; instances and neighboring copies provide visual context. A selected prototype exposes vertex handles. Draw mode accumulates vertices until a double-click or explicit close action. Vertex drag updates the draft through an undoable command. Secondary-click removes the nearest vertex only when the editor is in vertex-edit mode.

Validation runs after each draft mutation. It combines existing polygon validation, edge pairing/seam checks, and coverage/overlap checks. Invalid edges and vertices use an error color, diagnostics name the issue and index, and Apply is disabled while any fatal issue remains. A valid arbitrary simple polygon, including mixed regular shapes such as an octagon and square, is rendered without assuming a rectangular cell.

### 4. Kernel editor

Materialize `KernelDefinition` values for display, including expression-defined kernels. Render a heatmap centered on the anchor. The legend reports the numeric range; mask-disabled cells are visibly distinct from zero-valued enabled cells. The selected cell has a crosshair and numeric value label in the Inspector. Mouse drag paints a configurable value (default selected value); secondary drag clears/zeros; wheel zooms; middle drag pans.

Inspector fields are typed and visible: kernel name/symbol, width, height, anchor X/Y, normalization, source channel, target channel, selected cell coordinates/value, and expression parameters. `Tab` cycles fields; arrows and `+/-` adjust numeric fields; Enter commits; Escape cancels. Dimension edits resize values and masks deterministically, clamp anchors, and create one undo command. Adding/removing kernels updates each growth source's input list atomically and displays an arity diagnostic until fixed.

### 5. Growth editor and graph

Extend `TextBuffer` with selection, word movement, line/column queries, and a stable cursor representation. The editor widget draws a line-number gutter, styled tokens from the supported parser/typechecker, current-line background, cursor, selection, and diagnostic ranges. Key handling is focus-aware: printable characters, arrows, Home/End, page movement, Backspace/Delete, Enter, Ctrl-based word movement, and Escape to leave editing. Changes schedule a debounced compile/plot refresh; diagnostics never destroy the last valid plot.

Rasterize plot data to an RGBA image with fixed padding, axes, tick labels, grid, curve(s), zero line, invalid sample markers, and selected sample crosshair. For multiple kernel inputs, the selected input and pinned parameters are shown in a legend. Plot colors follow channel colors. Kitty receives the image directly; the half-block fallback uses the identical raster and preserves the graph's geometry at the available terminal resolution.

### 6. Input and C/S protocol

Mouse Down, Drag, and Up are all forwarded through `InputMessage::Mouse`, including middle-button events that currently only mutate `MouseTracker` state locally. The server-side tracker receives the middle Down before the first Drag and clears state on Up or focus loss. Input sequence acknowledgements cover gestures and Apply requests. The client must not claim a pan/paint succeeded solely because it changed a local camera or draft; E2E success requires an ack and, where applicable, an authoritative snapshot revision.

The same event-to-command path is used for Kitty and half-block modes. A resize invalidates the surface transform and forces one full redraw. On disconnect, the client cancels pending preview work, deletes the presented Kitty placement, and returns to a clean terminal state.

## Error handling and safety

- Invalid polygon, kernel, or growth edits remain visible as drafts with actionable diagnostics; they cannot silently modify the running server.
- A failed Kitty allocation, unsupported protocol, or stale shared-memory frame falls back to half-block without losing the current draft or focus.
- Kitty placement IDs are deleted at presentation time and on mode exit/fallback. Retained shared-memory objects are reaped only after terminal consumption or timeout according to the existing protocol safeguards.
- Numeric input rejects non-finite values and reports range errors in the Inspector.
- Repeated rapid edits coalesce preview work but never coalesce input commands or acknowledgements.

## Testing strategy

### Unit and model tests (remote only)

- Tiling coordinate transforms, polygon draw/drag/delete, self-intersection and coverage diagnostics.
- Kernel heatmap normalization, mask/anchor rendering coordinates, resize/anchor clamping, and growth-input arity invariants.
- TextBuffer cursor/selection/UTF-8 movement, syntax spans, diagnostics, and plot raster bounds.
- Mouse protocol round trips for middle Down/Drag/Up and focus-loss cleanup.

### User-level PTY tests on tinker

The harness downloads a pinned GitHub Release asset before each clean run, verifies SHA-256 against `SHA256SUMS`, and records the tag and version. It launches the actual release server/client under a PTY (not a fake connector), sends SGR mouse sequences and keyboard sequences, captures terminal output, and waits for protocol acknowledgements.

For Kitty mode it parses Kitty APC commands/shared-memory frames and checks pixel probes for the tiling polygon, kernel heatmap/anchor, growth axes/curve, cursor, selection, and placement deletion. For half-block mode it reconstructs the terminal cell image and checks the same logical probes at the lower resolution. Tests cover: entering Workbench with `W`, clicking every section, drawing and editing a polygon, changing a kernel dimension/value, editing a growth expression and seeing a changed graph, Apply/Revert, middle pan, rapid repeated gestures, resize, mode fallback, reconnect, and clean exit.

The report must distinguish server simulation rate, snapshot receive rate, UI draw rate, and fresh preview/Kitty consume rate. It must fail if an operation is only reflected in an optimistic client state, if a frame is stale, if an image placement accumulates, or if the process becomes unresponsive during a stress sequence.

## Scope boundaries

This design does not introduce GPU rendering for Workbench previews, a new expression language, or a new simulation backend. It improves the existing expression language's editing experience, uses CPU rasterization for editor previews, and keeps the server's simulation backend unchanged.
