# Basis-aware Workbench and Visual Agentic Validation Design

**Status:** Approved in conversation; implementation has not started.

## Relationship to earlier designs

This specification refines and, where they conflict, supersedes the Workbench,
tiling, kernel, growth-editor, and end-to-end testing sections of:

- `2026-08-21-interactive-experiment-workbench-design.md`
- `2026-08-22-workbench-graphics-editors-design.md`

The existing direct simulation mode, C1 client/server mode, release workflow,
Kitty shared-memory transport, input acknowledgements, and GPU simulation
architecture remain compatibility requirements.

## Problem

The implemented Workbench exposes concepts but does not yet behave like a
visual construction tool. Users cannot discover or reliably perform the core
tasks: drawing a non-rectangular periodic tiling, editing floating-point kernel
weights and metadata, or writing a growth program while seeing its full
signature and a precise plot. Earlier PTY-oriented tests exercised escape
sequences and internal state, but did not reproduce a user's visual journey.
They consequently missed stale Kitty placements, coordinate mismatches,
unclear focus, ineffective controls, cropped text, and freezes after repeated
interaction.

## Goals

1. Make polygon tiling, kernel, channel, and growth editing visually
   discoverable and directly manipulable.
2. Represent a periodic tiling containing one or more independent polygons per
   translation cell without confusing those polygons with channels.
3. Give every basis polygon independent kernels and growth behavior while
   preserving translation invariance across its periodic copies.
4. Provide low-burden defaults: one channel, one kernel, inherited RuleSets,
   and explicit additions only.
5. Validate edge-to-edge periodic coverage, including permitted T-junctions,
   for complex straight-sided polygon patches.
6. Render all editors as high-resolution Kitty graphics and keep the
   half-block fallback fully interactive.
7. Establish a real visual agentic test environment that sees final Kitty
   pixels and drives keyboard and mouse input like a user.
8. Preserve direct rendering for GPU-equipped local machines and C/S rendering
   for the weak ARM64 client connected to tinker.

## Non-goals

- Curved polygon edges, holes, aperiodic tilings, or automatic discovery of a
  fundamental period for every possible tiling.
- A general-purpose scripting language with mutation, loops, recursion,
  allocation, I/O, or external calls.
- Treating the Raspberry Pi's virtual-display rendering rate as a product
  performance measurement.
- Replacing server-side CUDA simulation with client-side computation.
- Using hidden test overlays or internal state as substitutes for what a user
  can see and operate.

## Mathematical and domain model

### Periodic basis

Two non-collinear translation vectors `a` and `b` define the periodic lattice.
A finite fundamental patch contains one or more **basis polygons**. Polygon
`alpha` translated by integer lattice coordinate `n = (i, j)` is a visible
tile instance, but all translated instances share the definition of basis
polygon `alpha`.

The state is:

```text
x[n, alpha, c]
```

where `c` is a channel. Basis polygon and channel are independent axes:

- every basis polygon has every channel;
- translated copies of a basis polygon share topology, kernels, and growth
  definitions;
- different basis polygons may have independent behavior even when their
  geometry is identical;
- channels remain scalar fields rather than tile types.

A new experiment starts with one basis polygon, one channel, and one kernel.
Additional polygons, channels, and kernels appear only through explicit Add
actions.

### Per-polygon RuleSets

Each `(target_basis, output_channel)` pair owns a RuleSet:

```text
RuleSet
  id
  target_basis_id
  output_channel_id
  kernels[]
  growth_program
  parameters[]
  update_mode
```

Every kernel within the RuleSet produces exactly one scalar input to that
RuleSet's growth program. Therefore the number of ordinary growth inputs is
always equal to the number of kernels in the RuleSet. `self` and named
parameters are visible inputs but do not count as kernels.

A kernel targets one basis polygon/output channel, selects one source channel,
and samples weights indexed by:

```text
(lattice_offset, source_basis, source_channel)
```

The `source_basis` axis is a per-basis weight bank, not a single source-basis
selector. A kernel may enable or mask each source basis independently while
producing one combined scalar potential. The spatial shape unit is the
translation cell, so translating the stencil never deforms it. Numeric weight
units are the actual source polygons inside each sampled cell. The preview must
draw those polygons in their real tiling geometry; a square matrix is not an
authoritative representation for a hexagonal or mixed-polygon tiling.

### Default inheritance and sharing

To avoid multiplying manual work, each output channel has a default RuleSet.
A newly added basis polygon inherits that default as a shared reference.

- Editing an inherited RuleSet locally performs copy-on-write and reports that
  the polygon is now detached.
- **Edit default** intentionally updates every still-inherited polygon.
- **Reset to default** discards a local override and relinks it.
- Named sharing groups are optional and operate on complete RuleSets.

Sharing is never applied to only the kernels or only the growth source. The
whole RuleSet moves together, preserving kernel count, input signature,
parameters, and update semantics.

## Workbench shell

The approved layout has three persistent regions:

1. an outline on the left for navigation and selection;
2. a large specialized graphics editor in the center;
3. a contextual inspector on the right for metadata, precise values, and
   diagnostics.

The central canvas is always the primary editor. The inspector must not contain
the only usable version of source code, a kernel preview, or essential controls.
Clicking an outline entry changes both center and inspector. `Tab` moves focus
between regions, arrows navigate within a region, and every mouse operation has
a visible keyboard equivalent. Focus, selection, active tool, and draft state
are visually distinct.

The two-row footer fits complete priority-ranked segments into its rectangle.
It never writes beyond its width or clips a glyph. `?` opens context-specific
help that explains the current canvas purpose and its gestures.

## Tiling editor

### Visual model

The selected basis polygon is drawn strongly at the center and is the only
directly editable copy. One complete **topological neighbor ring** is rendered
around it with reduced opacity. Neighbors are produced from confirmed seam
relations and periodic translations, not from rectangular horizontal/vertical
duplication. A regular hexagon therefore has six geometrically correct adjacent
copies. Panning and zooming never change the selected canonical basis polygon.

Overlays independently show:

- lattice vectors and fundamental-patch boundary;
- polygon IDs and prototype sharing;
- confirmed, suggested, unmatched, and invalid seams;
- atomic edges created by T-junction splitting;
- gaps, positive-area overlap, and periodic-copy provenance.

### Construction interaction

Polygon creation uses click-to-place vertices rather than freehand smoothing:

- click adds a vertex;
- pointer motion previews the next edge;
- double-click, Enter, or clicking the first vertex closes the polygon;
- dragging a handle moves a vertex;
- secondary click opens remove/split/numeric actions;
- middle drag pans and empty-canvas wheel zooms.

The user freely draws the central polygon and then drags or spawns neighbors.
The editor proposes adjacency by matching translated, rotated, or reflected
edge geometry. The user confirms each proposal; confirmation creates a
topological constraint rather than relying on later fuzzy coordinate matching.
A neighbor endpoint may snap to the interior of a longer edge, explicitly
creating a permitted T-junction. Translation vectors are inferred as
suggestions but remain user-confirmed numeric fields.

Common square, triangle, regular-hexagon, honeycomb, and octagon-square presets
are shortcuts into the same editable representation, not special simulation
types. Presets never limit custom geometry.

### Authoritative validation

Validation operates on a periodic arrangement on the quotient torus:

1. Use inverse lattice coordinates and canvas/fundamental-patch bounds to derive
   the necessary finite set of periodic copies. Fixed `-1..1` or `-2..2`
   neighborhoods are forbidden.
2. Insert all segment endpoints, proper intersections, T endpoints, and
   collinear-overlap endpoints. Split shape boundaries into canonical atomic
   edges. A T-junction is valid only after the long edge is split at the T
   endpoint.
3. Reject proper boundary crossings and positive-length incompatible overlaps.
4. Build a half-edge/DCEL arrangement with integer periodic offsets. Every
   atomic boundary half-edge must have exactly one oppositely oriented twin.
5. Check coverage multiplicity exactly once over the fundamental patch: no
   positive-area overlaps and no gaps.
6. Cross-check the sum of representative face areas with `abs(det(a,b))` and
   the torus Euler characteristic `V - E + F = 0`.

Orientation and intersection decisions use adaptive robust predicates rather
than a single fixed `f64` epsilon. Scale-aware tolerances apply only to metric
acceptance and display, not to contradictory topology.

Rendering and validation enforce hard budgets before expensive work: bounded
vertices per polygon, total atomic edges, candidate copies, clipping work, and
diagnostic count. The selected canonical polygon is always rendered but also
consumes the same global edge budget. Budget exhaustion yields an actionable
diagnostic instead of freezing the UI.

## Kernel editor

Selecting a basis polygon and output channel selects its RuleSet. The central
canvas displays the selected kernel over actual surrounding translation cells
and source polygons. The target polygon is outlined distinctly, and source
channel colors identify routing without using color alone.

Interactions:

- click selects one source-polygon weight;
- drag paints the current value;
- wheel over a weight adjusts its floating-point value;
- `Shift+wheel` uses a fine step and `Ctrl+wheel` a coarse step;
- double-click, Enter, or `E` opens an inline numeric editor;
- arrows move the selection geometrically;
- secondary click exposes reset, mask, copy, and paste;
- middle drag pans; wheel over empty space zooms;
- Add/Remove explicitly changes kernel count.

The inline editor has a cursor, selection, validation, commit, and cancel. The
inspector exposes kernel name and stable symbol, source channel, enabled source
bases, target basis/channel, lattice stencil extent, anchor, normalization,
numeric range, symmetry/mask state, paint value, and adjustment steps. Large
stencils must remain fully reachable through zoom/pan and keyboard selection
rather than being irreversibly downsampled.

Adding, removing, or reordering a kernel regenerates the read-only growth
signature immediately. Stable symbols survive reordering. Removed symbols
produce source diagnostics; new unused symbols produce warnings.

## Growth editor

The generated signature is always visible above the editable body in the
central canvas. For one polygon, one channel, and one kernel it is conceptually:

```text
target: hexagon_A / state
fn growth(self: Scalar, potential_0: Scalar) -> Rate
```

Parameters such as `mu` and `sigma` are shown beside the signature and remain
named program variables. The user edits only the function body. The language
supports immutable `let` bindings, `if/else` expressions, arithmetic,
comparisons, booleans, comments, and a bounded scalar math library. The final
expression is the result; loops, mutation, recursion, allocation, I/O, and
external calls remain forbidden.

The editor provides a visible cursor, selection highlight, line numbers,
syntax highlighting, bracket matching, scrolling, word movement, Undo/Redo,
completion, and span-linked diagnostics. Final-source validation is tied to the
complete current text, not to any valid intermediate prefix.

The lower central canvas is a precise RGBA plot:

- one chosen input produces a curve with axes, grid, zero line, values, and
  selected sample;
- two chosen inputs produce a heatmap and contours;
- remaining kernel inputs, `self`, and parameters are pinned with visible
  controls;
- hovering reports all inputs, local bindings, selected branch, and result.

An invalid body preserves the last valid plot but marks it visibly stale and
shows diagnostics. Neither a stale marker nor a changed error decoration counts
as a successful live-plot update.

## Channels and presentation

The default experiment has one channel. Channel addition is explicit. Every
basis polygon then receives that channel and a corresponding inherited default
RuleSet.

Automatic colors prioritize clarity:

- one channel: near-white on pure black inside the domain;
- exactly three channels: red, green, and blue;
- other counts: a high-contrast accessible palette.

Colors are user-editable and custom choices are not overwritten by later
automatic palette changes. The exterior of the simulated domain keeps the
existing dark navy color. Composite, Solo, and Grid views retain exact Inspect
values independent of display quantization.

## Rendering and input architecture

All Workbench editors produce a bounded RGBA scene independent of terminal
transport. Kitty receives the scene through the existing graphics lifecycle;
half-block converts the same pixels and uses the same scene-to-terminal
transform. Switching section, mode, fallback, resize, disconnect, or exit must
delete the currently presented Kitty placement before presenting another.

One authoritative transform maps terminal mouse coordinates to logical scene
coordinates. It includes region origin, cell pixel dimensions, image placement
offset, scale, pan, and resize generation. Rendering and hit-testing consume
the same immutable transform so painted and displayed positions cannot diverge.

Preview work is latest-only, cancellable, and event-driven. Draft mutations,
selection, camera, text cursor, and resize invalidate a generation. Repeated
draws of an unchanged image do not increment the fresh-graphics metric. Input
reception, server stepping, preview rasterization, and terminal presentation
remain decoupled so a slow stage cannot block keyboard or mouse handling.

## Transaction, persistence, and C/S behavior

Workbench edits remain local in an `ExperimentDraft` based on an authoritative
revision. Apply sends the complete basis-aware experiment to the server for
robust validation, sparse-weight compilation, backend compilation, and a
non-committing test step. Success atomically swaps the experiment and returns a
normalized authoritative model. Failure preserves the running experiment and
the draft.

Input sequence acknowledgements cover every gesture and edit command. Visual
optimism may improve responsiveness but never proves that a remote action
succeeded. Reconnect preserves the original base revision and exposes conflicts
rather than silently rebasing or overwriting server state.

The versioned wire and persistence schema uses stable IDs for basis polygons,
channels, RuleSets, kernels, and symbols. Legacy single-channel/single-kernel
experiments migrate to one basis polygon, one channel, one kernel named
`potential`, and one growth program. Unknown newer versions remain hard errors.

Direct mode invokes the same validation and compilation APIs in process. It is
never removed or forced through SSH.

## Performance boundaries

The weak ARM64 machine performs only input, bounded editor rasterization,
terminal presentation, and protocol work. It never runs Cellarium simulation
performance tests or builds Cellarium from source. Server validation,
compilation, simulation, and performance measurement run on tinker's NVIDIA
backend.

Metrics use distinct clocks and labels:

- server simulation completions per second;
- authoritative snapshots received per second;
- UI draws per second;
- fresh Workbench/simulation graphics produced per second;
- Kitty images actually consumed/presented per second;
- input-to-server-ack and input-to-visible-change latency.

Rates use fixed wall-clock windows including idle tails. Re-presenting a stale
frame cannot increase fresh-frame or consume rates.

## Visual agentic validation

### Headless real-Kitty environment

The Raspberry Pi runs no heavyweight desktop. Each test creates an isolated
virtual X11 session containing only:

```text
Xvfb -> Openbox -> Kitty -> released ARM64 Cellarium client -> tinker server
```

The installed runtime tools are `Xvfb`, `openbox`, `kitty`, `xdotool`, and
`ffmpeg`. No GNOME, KDE, display manager, or physical monitor is required.
Kitty renders final pixels into the virtual X framebuffer. `xdotool` supplies
real X11 key, pointer, drag, middle-button, double-click, and wheel events.
`ffmpeg` captures the complete framebuffer for visual inspection.

Every clean journey:

1. creates a private X display and XDG cache/config/runtime directories;
2. records display geometry, DPI, font/config, Kitty version, release tag,
   asset URL, SHA-256, and Cellarium version;
3. downloads the prebuilt GitHub Release ARM64 binary and verifies it against
   `SHA256SUMS`; it never performs a local build;
4. installs/starts the matching released server binary in
   `~/.local/bin/cellarium` on tinker and records its hash/version;
5. opens a dedicated Kitty window and runs `cellarium connect tinker` inline;
6. performs the journey through screenshots and visible controls;
7. terminates the complete process group and confirms no client or server
   process, X socket, shared-memory object, or Kitty placement leaked.

The virtual display is authoritative for functional visual testing, mouse
mapping, layout, Kitty placement lifecycle, and interaction semantics. Its
software-rendered frame rate is not used to characterize real-device Kitty
performance.

### Agent behavior

This is not a prerecorded coordinate macro. For every step the Agent:

1. captures the current full window;
2. uses visual understanding to identify the intended visible control or
   geometry;
3. performs a user-like mouse or keyboard action;
4. waits for the action's server acknowledgement when applicable and for a
   correlated new visual generation;
5. captures and interprets the result before choosing the next action.

Coordinates are derived from the current screenshot, not hard-coded terminal
cells. Test-only traces may correlate input sequence, server acknowledgement,
revision, and fresh generation, but cannot serve as the visual pass condition.
Internal optimistic state, the appearance of an error marker, or an unrelated
delayed frame never counts as success.

### Required user journeys

The visual Agent must complete and retain before/after evidence for:

1. Start from Simulation and discover Workbench without a memorized shortcut.
2. Navigate every outline section by mouse and keyboard; verify focus and help.
3. In Tiling, draw and close a custom non-axis-aligned polygon, move/delete/add
   vertices, construct and confirm neighbors, create a T-junction, inspect the
   ghosted neighbor ring, pan, zoom, Undo, and Redo.
4. Load regular hexagon and octagon-square fixtures and visually verify their
   non-rectangular adjacency; deliberately create overlap, gap, crossing, and
   unmatched-seam errors and then repair them.
5. Select different basis polygons and confirm translated copies map back to
   the same basis while different bases expose independent RuleSets.
6. In Channels, retain the default one-channel palette, explicitly add two
   channels, verify RGB composite/solo/grid views, customize one color, and
   confirm pure-black in-domain zero versus dark-navy exterior.
7. In Kernel, select an actual source polygon, adjust a float with normal/fine/
   coarse wheel steps, enter an exact value through the inline editor, paint,
   clear, mask, pan, zoom, navigate by keyboard, change metadata, add a second
   kernel, and verify the growth signature gains exactly one input.
8. Detach one inherited RuleSet, edit it, verify siblings remain unchanged,
   edit a default, and relink through Reset to default.
9. In Growth, see the complete target and generated signature, edit multiline
   `let` and `if/else` source with cursor/selection/highlighting, create and fix
   a diagnostic, verify the final valid source changes the curve, inspect a
   sample, select a second input for a 2D heatmap, and pin remaining variables.
10. Apply, observe authoritative revision/state, return to Simulation, and
    verify the Workbench image is fully deleted rather than overlaid.
11. Re-enter, Revert, resize repeatedly, switch editors rapidly, reconnect, and
    run a sustained mixed keyboard/mouse stress sequence without freeze,
    coordinate drift, stale frames, cropped footer text, or leaked processes.
12. Repeat the functional journey in half-block mode, including navigation,
    paint, numeric edit, text edit, pan, zoom, Apply, and clean mode exit.

The Agent also records newly discovered usability defects that are not encoded
in assertions. A journey fails when the Agent cannot discover how to proceed,
even if an internal command technically exists.

## Automated verification supporting the Agent

Remote unit/property tests cover robust predicates, segment splitting,
periodic DCEL invariants, coverage multiplicity, budgets, basis/channel/RuleSet
serialization, kernel arity, inheritance, scene transforms, graphics placement
deletion, text editing, and plot sampling. Protocol tests bind actions to input
sequence acknowledgements and authoritative revisions.

These tests support but do not replace the visual journey. A release is not
ready when only unit, PTY, Kitty-command parsing, or screenshot-hash tests pass.
The retained agentic report includes annotated screenshots, action intent,
observed result, acknowledgement/revision correlation, latency, process cleanup,
and a pass/fail judgment for every journey step.

## Error recovery and safety

- Invalid edits remain visible and undoable; they never mutate the active
  simulation.
- A failed Kitty frame falls back without losing draft, focus, or input.
- A failed Xvfb/Kitty launch aborts the journey with logs; it does not silently
  substitute a PTY-only test.
- Cleanup addresses only processes and temporary resources created by that
  journey, using recorded identities rather than broad process-name killing.
- Server startup uses a unique test session and always verifies termination,
  preventing the previous accumulation of orphaned Cellarium servers.
- Resource and geometry budgets turn pathological input into diagnostics, not
  unbounded work or UI freezes.

## Delivery order

1. Establish the isolated Xvfb/Openbox/Kitty visual harness and run the current
   released client/server journey as a retained failing UX baseline.
2. Introduce the versioned basis/channel/RuleSet model, migrations,
   copy-on-write defaults, kernel arity invariant, and authoritative C/S
   round-trip without changing the simulation renderer.
3. Replace whole-edge/fixed-neighborhood tiling validation with atomic-edge
   splitting, robust periodic arrangement checks, budgets, and known valid and
   invalid fixtures.
4. Implement the shared scene transform and the specialized Tiling, Kernel,
   Growth, and Channel graphics editors, retaining direct mode and half-block
   input behavior throughout.
5. Complete authoritative Apply/metadata synchronization, lifecycle cleanup,
   correlated metrics, and the entire visual agentic journey. Fix every
   discovered blocker and usability defect before declaring the release ready.
6. Build and publish through CI, download the published ARM64 artifact for the
   clean final journey, install the matching tinker server in `.local/bin`, and
   release only after the retained evidence passes.

## Acceptance criteria

The feature is ready only when:

1. all required remote unit, integration, protocol, and release checks pass;
2. every visual agentic journey above passes in a clean Kitty session and the
   functional fallback journey passes in half-block mode;
3. no required action depends on undocumented knowledge or an invisible focus;
4. visual position and hit-testing agree after pan, zoom, resize, and mode
   changes;
5. the final growth source, not an intermediate prefix or stale decoration,
   drives the displayed plot;
6. Apply is proven by server acknowledgement and authoritative revision;
7. leaving/switching editors removes previous Kitty images completely;
8. stress runs finish responsively and leave no client/server processes or
   graphics resources;
9. the release uses prebuilt artifacts on ARM64 and performance claims come
   only from the appropriate tinker/real-terminal environment.

## References

- CGAL Arrangement_on_surface_2, including arrangements on parametric
  surfaces: <https://doc.cgal.org/latest/Arrangement_on_surface_2/index.html>
- J. R. Shewchuk, adaptive-precision orientation and incircle predicates:
  <https://people.eecs.berkeley.edu/~jrs/papers/robust-predicates.pdf>
- C. S. Kaplan and D. H. Salesin, Escherization:
  <https://grail.cs.washington.edu/wp-content/uploads/2015/08/kaplan_siggraph2000.pdf>
- Periodic tilings represented with integer translation data:
  <https://discovery.ucl.ac.uk/id/eprint/10121654/1/sotosanchez2021integer.pdf>
- XDG Remote Desktop Portal (an alternative for testing a real external desktop
  rather than the selected headless X11 harness):
  <https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html>

