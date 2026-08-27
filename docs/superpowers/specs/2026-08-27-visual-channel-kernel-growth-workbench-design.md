# Visual Channel, Kernel, and Growth Workbench Design

**Date:** 2026-08-27
**Status:** proposed for implementation
**Product baseline:** Cellarium v0.2.2 / commit `271b79e`
**Feature inventory:** `docs/feature-inventory.md`

## 1. Purpose

Redesign the Channels, Kernels, and Growth sections so a user can discover,
understand, and complete their main tasks primarily through visible graphics
and mouse interaction.

The work also replaces shallow interaction checks with complete agentic user
journeys. A journey succeeds only when the Agent observes the intended result,
not merely when an input event is accepted or a draft counter changes.

## 2. Problems established from real use

### 2.1 Channels is a misleading state viewer

When a structural draft does not match the running experiment, Channels maps
the draft's initialization across the new lattice and fills the main canvas
with it. For an oblique lattice this appears as a skewed parallelogram of
high-frequency noise. The page does not visually prioritize its actual task:
managing channels, colors, visibility, freezing, and composition.

The label `Preview: draft initialization` is technically accurate but does
not make the result understandable or actionable.

### 2.2 Kernels has no usable collection interaction

The selected kernel occupies the whole canvas, while the rest of the RuleSet
exists only as a count and a `]` shortcut. There is no visual list, position,
thumbnail, or direct mouse target for another kernel.

Add, next, and delete are adjacent text toolbar commands with weak feedback.
In the reported workspace the selected RuleSet reached twenty kernels while
the user was trying to navigate and delete. Whether created by accidental
activation or repeated uncertain input, the interface failed to make the
mutation visible and reversible at the point of action.

Deletion also attempts to remove the selected kernel but may reject it because
the Growth source refers to that symbol. The rejection is shown as transient
text rather than a visible decision.

### 2.3 Growth chooses visualization dimension from arity, not intent

The current editor chooses a curve for fewer than two kernel inputs and a
heatmap for two or more. A RuleSet with twenty kernels therefore becomes a
heatmap even when its program only reads `potential`. The default y-axis is
`k1`, which contributes nothing to the function.

The source, signature, plot caption, diagnostics, syntax reference, and a long
argument list compete for space. An empty plot gives no actionable explanation.

### 2.4 Inspector mixes scopes

The generic Inspector places global channel count beside the current
Binding's kernel count. It then appends dense shortcut prose and detailed
kernel or language metadata. The user cannot tell which values are global,
which belong to the selected Binding, or what to do next.

## 3. Product principles

1. **The main canvas represents the section's primary task.**
2. **Collections are visible collections.** Channels and kernels must be
   directly selectable by mouse, not hidden behind a next shortcut.
3. **Every mutation has local visual feedback.** Add, delete, freeze, hide,
   share, detach, and edit change the object where the user acted.
4. **Text explains exceptions; graphics carry ordinary interaction.**
5. **Keyboard shortcuts remain accelerators, not the only discoverable path.**
6. **No silent destructive repair.** Existing twenty-kernel workspaces are
   preserved until the user explicitly deletes or resets them.
7. **Draft, running, and unavailable state are visually distinct.**
8. **Agentic validation follows user goals through completion and recovery.**

## 4. Shared interaction framework

### 4.1 Three visual regions

The existing Outline / Canvas / Inspector shell remains. Within Canvas, the
three redesigned sections use:

1. a compact object strip at the top;
2. a large primary visual editor;
3. a short contextual action/status row.

The object strip wraps or scrolls horizontally when it does not fit. It never
silently drops objects.

### 4.2 Mouse and keyboard parity

Every primary collection action has a mouse target:

- select object by clicking its card;
- add by clicking the trailing plus card;
- delete by clicking the selected card's delete affordance;
- change a boolean by clicking its icon;
- choose source/output/axis by clicking a chip;
- edit an exact value by double-clicking the value or selected cell.

Existing shortcuts remain and operate on the same controller actions. Mouse
and keyboard routes must produce identical draft and selection states.

### 4.3 Persistent selection

Selection uses stable IDs, never vector positions. After add/delete/undo/redo:

- a surviving selected ID stays selected;
- deleting the selected object selects its nearest surviving neighbor;
- deleting the last permitted object is rejected without changing selection;
- undo restores both object and selection;
- redo restores the post-action selection;
- no Inspector state may display a dangling selected ID.

### 4.4 Action feedback

Successful actions animate or highlight the affected card for a short,
generation-based interval and update a persistent status line.

Rejected actions open an in-canvas decision/error panel anchored to the
affected card. Errors are not limited to a footer that disappears on the next
input.

## 5. Channels design

### 5.1 Channel card strip

Each channel card contains:

- channel name;
- color swatch;
- visible/hidden eye icon;
- active/frozen state;
- selected border;
- delete affordance when deletion is legal.

The trailing `+` card adds one channel. A one-channel experiment therefore
shows one real card and one unambiguous add target.

Clicking a card selects it. Clicking the swatch opens the exact RGB editor;
clicking the eye toggles visibility; clicking active/frozen toggles update
state; clicking delete requests deletion.

### 5.2 Primary preview

The large preview has explicit modes selected by visual tabs:

- **Composite** — all visible channels composited;
- **Solo** — selected channel only;
- **Grid** — one labelled panel per channel.

The preview uses the same polygon scene and camera transform as Simulation.
It is not a rectangular placeholder and does not use random data.

### 5.3 Running versus draft

If the draft structure matches the running experiment, the default source is
**Live** and the image updates from authoritative snapshots.

If the structure differs:

- the canvas does not silently substitute a full-screen draft initialization;
- a clear two-state header shows `Live: old structure` and
  `Draft: not applied`;
- the default selected tab is **Draft** because the user is editing it;
- the draft preview is fitted and labelled `initial state`;
- if the initialization is visually dense, it remains truthful but occupies a
  bounded preview with an explanatory overlay rather than masquerading as a
  live result;
- an **Apply & Run** button is visible beside the state tabs.

The Live tab remains available as a read-only comparison using the old
authoritative geometry.

### 5.4 Counts and scope

The Channels Inspector shows:

- total channels;
- active channels;
- frozen channels;
- selected channel identity;
- selected display mode;
- selected color and visibility.

It does not show the selected Binding's kernel count. That belongs to Kernels
and Experiment summaries.

### 5.5 Lifecycle correctness

Channel IDs and default names are derived from the maximum existing ID plus
one, not current vector length. Delete/add cannot create duplicate IDs or
names.

Freeze removes or disables every normalized Binding whose output is the
frozen channel. Unfreeze creates the required default Binding for every basis.
Undo/redo restores the complete normalized RuleLibrary atomically.

## 6. Kernels design

### 6.1 Kernel thumbnail strip

The top strip shows every kernel in the selected
`(basis polygon, output channel)` RuleSet.

Each card contains:

- ordinal and symbol, for example `2 · k1`;
- a small color thumbnail of its actual support and weights;
- source channel chip;
- selected border;
- delete affordance.

The header displays `Kernel 2 of 4`, not only `kernels: 4`.

The trailing plus card adds a kernel and selects it. Adding one kernel must
produce a new visible card before any further input.

### 6.2 Direct selection and deletion

Clicking any card selects that exact kernel and updates the large editor,
Inspector, Growth signature, and selection ordinal.

Clicking delete:

1. rejects deletion if this is the RuleSet's only kernel;
2. checks whether the Growth source actually references the kernel symbol;
3. if unreferenced, deletes immediately;
4. if referenced, opens a decision panel:
   - **Replace references with 0 and remove** — replaces every bound reference
     to that kernel symbol with the scalar literal `0`, recompiles the source,
     and deletes only if the resulting complete draft validates;
   - **Cancel** — leaves everything unchanged.

The panel names the referenced symbol and previews the exact source change.
The whole operation is one undoable transaction. If recompilation fails, the
kernel, source, and selection remain unchanged and the panel shows the
diagnostic. The interface never silently guesses another kernel or formula.

A separate **Reset RuleSet** action restores the selected Binding to its
shared/default RuleSet after confirmation. It is the intended recovery for an
accidentally expanded local override, including the reported twenty kernels.

### 6.3 Large graphical editor

The selected kernel remains a large high-resolution graphics editor:

- polygon cells reflect the actual periodic tiling;
- active positive, active negative, active zero, inactive, anchor, selected,
  and source basis states have a persistent visual legend;
- click selects;
- left drag paints;
- right drag zeros or deactivates according to the selected tool;
- wheel over an active cell changes its floating value;
- double-click opens exact value entry;
- middle drag pans;
- wheel over empty/inactive space zooms;
- Fit restores a useful whole-stencil view.

### 6.4 Tool palette

Weights and Support are visual toggle buttons, not only `M` text.

The palette also contains:

- Gaussian preset;
- sampling metric: Affine or World;
- sigma value;
- stencil size and anchor;
- source channel;
- output Binding;
- reset-to-default.

Clicking a numeric value opens the exact editor. Source and output use
clickable chips/lists.

### 6.5 Selection and mutation invariants

Add, select, edit, delete, undo, redo, detach, and reset must refresh:

- selected kernel ID;
- kernel card strip;
- large graphical scene;
- Inspector;
- Growth signature and external symbols;
- plot axes and data;
- draft validity.

No path may leave a selected ID that is absent from the selected RuleSet.

## 7. Growth design

### 7.1 Binding and input strip

The top of the Canvas shows:

- selected basis polygon card;
- output channel card;
- update mode: Rate or Value;
- one chip for `self`;
- one chip for each kernel input in RuleSet order.

Kernel chips use the same symbol, ordinal, and color identity as the Kernels
section. Clicking one opens that kernel in Kernels. This makes Growth and
Kernel arity visibly consistent.

### 7.2 Source editor

The source editor remains central and receives more horizontal space than the
Inspector. It shows:

- full function signature above the source;
- line numbers;
- visible cursor;
- selection highlight;
- syntax coloring;
- inline diagnostic underline/marker at the actual span;
- persistent valid/stale/error state.

The Inspector defaults to a concise context page. Syntax help is a separate
scrollable tab rather than an always-visible wall of text.

### 7.3 Axis selection

Visualization dimension is based on selected/referenced variables, not total
kernel count.

The compiler exposes the set of external scalar symbols referenced by the
typed program.

Default policy:

- zero referenced kernel inputs: 1D curve over `self`;
- one referenced kernel input: 1D curve over that input;
- two or more referenced kernel inputs: 2D heatmap using the first two
  referenced inputs in signature order.

The user may click any input chip to assign X, then another to assign Y.
Clicking the active Y chip removes Y and returns to a curve.

Unused inputs remain visible in the signature and chip strip but do not force
the plot into another dimension.

### 7.4 Plot behavior

Curve and heatmap are rendered as high-resolution RGBA graphics.

The plot always shows:

- axis names and numeric domain;
- output meaning, Value or Rate;
- zero reference/contour;
- pinned values for non-axis inputs;
- stale overlay when source is invalid;
- a visible message when no finite samples exist.

Isolated discontinuities such as equality tests are rendered with explicit
sample markers; they must not disappear into an apparently flat empty graph.

The plot domain remains editable through clickable min/max labels and
`d`/`D` shortcuts.

### 7.5 Multi-kernel consistency

After adding, deleting, reordering, sharing, detaching, or resetting kernels:

- the signature has exactly `self + selected RuleSet kernels`;
- the chip strip has the same number and order;
- referenced-symbol analysis is recomputed;
- invalid removed symbols produce a source diagnostic rather than a blank
  graph;
- selection of plot axes remains stable by symbol where possible.

## 8. Inspector redesign

Inspector content is section-specific and layered:

1. concise object summary;
2. current selection/action state;
3. one short next-action hint;
4. optional Help tab.

The default page does not repeat the entire toolbar or language manual.

Counts are labelled with scope:

- Channels: `all channels`, `active`, `frozen`;
- Kernels: `current Binding kernels`, `selected 2/4`,
  `all experiment effective kernels`;
- Growth: `Binding basis/channel`, `function kernel inputs`;
- Experiment: `basis × active channels = Growth bindings`.

## 9. Data safety and migration

No automatic migration deletes duplicate or surplus kernels from saved
workspaces. The redesigned UI exposes them and provides deliberate deletion
and Reset RuleSet.

Workspace format remains compatible unless a new persistent UI choice is
required. Transient selection, card scroll, help tab, and plot axes stay UI
state unless product behavior requires persistence.

Channel lifecycle repairs operate on a cloned ExperimentSpec, validate the
complete model, and replace the draft atomically. Failure leaves the original
draft and selection unchanged.

## 10. Rendering and performance

Object cards and controls are ordinary TUI cells; thumbnails and large
editors use the existing graphics pipeline.

Thumbnail generation is cached by stable object ID and content generation.
Changing selection does not recompute every thumbnail. A latest-only worker
may replace stale thumbnail work, but the selected large editor has priority.

Section transitions continue to delete the previous Kitty placement before
presenting incompatible content. Half-block renders the same logical scenes
and retains all interaction, with reduced precision only.

No performance conclusion is drawn from the Raspberry Pi's software Xvfb.
Server simulation and GPU performance remain measured on tinker. Agentic
tests on the Pi measure functional responsiveness and visible state, not GPU
throughput.

## 11. Agentic user testing contract

### 11.1 Fidelity

The final gate runs a stable, precompiled ARM64 Release client on the
Raspberry Pi in an isolated:

`Xvfb → Openbox → Kitty → cellarium connect tinker`

session. The server runs the matching installed binary on tinker.

The Agent:

1. observes the current framebuffer;
2. chooses the next coordinates from that image;
3. sends real X11 mouse/keyboard input;
4. observes the new framebuffer;
5. records whether the user-visible goal was achieved;
6. adapts the next action to the observed state.

Static code, unit tests, PTY bytes, trace lines, counters, image hashes, or
successful event injection never replace this judgment.

### 11.2 Completeness rule

Every feature in `docs/feature-inventory.md` receives a journey row with:

- natural entry path;
- realistic user action;
- expected visible result;
- cancel/error path where applicable;
- undo/redo where the action mutates a draft;
- Apply/persistence check where the action changes the experiment;
- before/after evidence;
- Agent visual judgment.

A feature with only an input receipt or internal state assertion is
**untested**, not passed.

### 11.3 Mandatory Channels journey

From a clean one-channel experiment:

1. enter Channels by clicking the Outline;
2. identify Live/Draft state without reading source or traces;
3. add two channels by clicking the plus card;
4. verify three visible cards and RGB default colors;
5. click each card and confirm selected highlight and Solo preview;
6. change one exact color by mouse;
7. hide/show another channel and observe Composite change;
8. freeze/unfreeze the third and verify active/frozen count and Binding count;
9. delete the middle channel and verify stable remaining selection;
10. add another channel and verify unique ID/name;
11. undo and redo delete/add;
12. switch Composite/Solo/Grid using visual tabs;
13. create an unapplied hexagonal or triangular structural draft and verify
    the Draft preview is understandable and the Live comparison remains
    accessible;
14. Apply & Run and verify the preview becomes Live.

### 11.4 Mandatory multi-kernel journey

From a Binding with one kernel:

1. click the plus card three times to reach four kernels;
2. after each add, verify a new card appears and is selected;
3. click each of the four cards in non-sequential order;
4. verify ordinal, symbol, source, thumbnail, large editor, and Inspector all
   refer to the clicked kernel;
5. edit each kernel into a visibly different pattern using mouse paint,
   support toggles, floating wheel adjustment, and exact value entry;
6. change one source channel and one sampling metric through mouse controls;
7. delete the second kernel by its card;
8. if referenced, exercise Cancel and verify no mutation, then resolve the
   reference through the offered safe path;
9. verify the deleted card disappears, nearest card is selected, Growth
   signature/chips shrink, and the plot remains meaningful;
10. undo deletion and verify the exact kernel/card/pattern returns;
11. redo and verify it disappears again;
12. reset a deliberately expanded local override to default and confirm the
    shared/default identity and kernel count;
13. Apply & Run, re-enter Workbench, and confirm the authoritative collection.

### 11.5 Mandatory Growth journey

1. select a one-input Binding from its visible chips;
2. edit a valid multiline `let` and `if/else` program with real text input;
3. verify cursor, selection, signature, diagnostics, and 1D plot;
4. enter invalid syntax, observe span diagnostic and stale plot, then repair;
5. add an unused second kernel and verify the plot stays 1D;
6. reference the second kernel and verify the plot becomes a 2D heatmap;
7. select x/y axes by clicking chips and observe labels/data change;
8. remove Y and verify return to a curve;
9. test an equality/discontinuity program and verify isolated markers;
10. switch Rate/Value and verify signature, caption, and update explanation;
11. edit min/max using mouse exact editors;
12. scroll the syntax Help tab and return to context without losing source;
13. Apply & Run and verify the program affects the live experiment.

### 11.6 Whole-product journey

The test does not stop at the three redesigned pages. It also exercises every
inventory item that has a user interaction surface:

- Simulation pause, step, reset, randomize, clear, paint, erase, pan, zoom,
  inspect, rule switch, Workbench enter/leave, and quit;
- Tiling blank start, all presets, free polygon drawing, illegal-point
  rejection, three close methods, undo/redo during construction, basis
  selection, vertex drag, pan/zoom/Fit, seam solve, linked drag, invalid
  gap/overlap/crossing, strict no-T-junction validation, and Apply;
- World channel selection, view, paint/erase, pan/zoom, undo/redo;
- Experiment dt edit, invalid Apply rejection, Apply & Run, Revert, Save,
  autosave, export, load, reconnect, and recovery;
- narrow/wide resize, section transition clearing, Kitty graphics, half-block
  fallback, direct Kitty SSH, C/S, and cleanup.

### 11.7 Stress and recovery

Run a sustained mixed-use session for at least ten minutes:

- repeatedly select non-adjacent channels and kernels;
- add/delete/undo/redo;
- change sections during pending graphics work;
- pan/zoom while snapshots arrive;
- resize;
- Apply more than once;
- disconnect/reconnect once.

Fail on stale images, blank plots without explanation, selection drift,
unbounded latency growth, frozen input, duplicate objects, unrecoverable
Invalid state, leaked process/image/shared-memory resources, or a control that
cannot be discovered and used from the visible interface.

### 11.8 Release gate

A stable release is allowed only when:

1. remote unit/integration/PTY/CUDA gates pass on tinker;
2. the exact release artifacts and checksums are identified;
3. the precompiled ARM64 artifact passes the full real-Kitty journey;
4. the critical journey passes in half-block;
5. every action has evidence and an Agent observation;
6. no unresolved defect remains;
7. test-owned Xvfb, Openbox, Kitty, client, server, image, and shared-memory
   resources are cleaned up.

## 12. Acceptance criteria

- Channels is primarily a visual channel manager, not an unexplained noise
  canvas.
- Every channel is directly selectable and mutable by mouse.
- Every kernel in the selected RuleSet is visible as a directly selectable
  card.
- Add, switch, edit, delete, undo, redo, and reset are visually closed-loop.
- Growth dimension follows referenced/chosen axes rather than total arity.
- A valid program never produces an unexplained blank plot.
- Inspector text is concise, scoped, and secondary to graphical interaction.
- Normalized Channel lifecycle operations preserve a valid, selectable model.
- The complete user-level agentic contract passes on the exact stable release.
