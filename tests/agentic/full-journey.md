# Certified basis Workbench visual journey

This checklist is executed against the exact ARM64 and x86_64 assets from a
draft GitHub Release. It is not a scripted screenshot test. For every action,
the Agent must capture the full framebuffer, visually locate the current
control from that frame, send one real X11 event, capture again, and write a
visual observation. Hashes, traces, and acknowledgements establish causality
only.

Any failed or ambiguous row fails the candidate. Preserve its evidence, fix the
product with a regression test on `tinker`, create a new draft candidate, and
restart at J01.

## Identity and environment

Record before J01:

- release tag and draft-release URL;
- ARM64 and Linux x86_64 archive names and SHA256 values;
- client/server `cellarium --version` and executable SHA256 values;
- Pi architecture, Kitty version, Xvfb display, window ID, dimensions, and
  session process group;
- tinker server PID/start time, NVIDIA device, and listening endpoint;
- protocol version and rendering mode.

The Pi must not compile Cellarium. Install only the checksum-verified ARM64
archive. Install the matching x86_64 archive at
`/home/wkj/.local/bin/cellarium` on `tinker`.

## C/S Kitty journey

Launch the released ARM64 client as `cellarium connect tinker`.

| ID | User action through visible UI | Visual acceptance |
|---|---|---|
| J01 | Observe launch, move pointer across viewport, pause/resume, paint and erase | Complete unclipped UI; pointer and paint agree; ack and visible response are bounded; simulation, snapshot, UI, fresh graphics and presentation metrics are distinct |
| J02 | Discover and activate Workbench from visible help/footer | Entry is discoverable; old simulation graphics are completely gone |
| J03 | Click World, Tiling, Channels, Kernels, Growth and Experiment in the left outline | Highlight follows every click; center and inspector both become contextual; no control is inert |
| J04 | In Tiling load Square, Equilateral Triangles, Regular Hexagon and Octagon-Square presets | Strong canonical polygon and one translucent topological neighbor ring are geometrically correct; hexagon has six non-axis-aligned neighbors; mixed octagon/square bases are visibly distinct |
| J05 | Select canonical and translated copies of each basis | Copies map to the same stable basis; different basis polygons change the target RuleSet shown by Kernel/Growth |
| J06 | Start a custom polygon; place non-axis-aligned vertices by clicking; move pointer to preview; close by Enter | Center polygon is directly editable, preview follows pointer, closure is clear, no rectangular preset is silently substituted |
| J07 | Drag a vertex, undo, redo; middle-drag and wheel on empty canvas | Geometry follows exact pointer; camera remains coherent; canonical selection is stable; interaction stays responsive |
| J08 | Create/position a neighbor, inspect seam suggestions, confirm a seam | Confirmed/unmatched/suggested states are visibly different and neighbor ring changes from actual seam topology |
| J09 | Create a T endpoint on a longer edge and confirm it | Long edge is visibly split into atomic edges; validation permits the explicit T-junction and reports no hidden gap/overlap |
| J10 | Intentionally create a gap, overlap and crossing, then repair each | Each defect has a precise visible diagnostic and Apply remains blocked until repaired |
| J11 | In Channels confirm initial count one; explicitly add channels two and three; toggle visibility and change one color | One channel uses high-contrast color on black, three channels default RGB, custom color applies only to intended channel, domain exterior keeps its distinct background |
| J12 | Return to Tiling and select two bases after adding channels | Every basis exposes all channels while basis and channel remain visibly separate concepts |
| J13 | In Kernels confirm initial kernel count one; click a real source polygon weight | Actual tiling geometry is shown, not a square proxy; target basis/channel, source basis/channel, stencil extent, anchor and value are visible |
| J14 | Wheel, Shift-wheel and Ctrl-wheel over the selected weight | Intended floating value changes by coarse/normal/fine increments without moving selection |
| J15 | Press E/Enter, edit an exact decimal with cursor/selection, commit; repeat with invalid input and cancel | Inline editor is visible, supports text selection, rejects invalid input, and preserves the previous value on cancel |
| J16 | Drag-paint several weights; middle-pan; zoom empty canvas; use keyboard to reach off-screen stencil cells | Paint is continuous and responsive; every large-stencil cell remains reachable; graphics and hit-test coordinates agree |
| J17 | Explicitly add a second kernel and change its source channel/weights | Kernel count becomes two, both previews are independently selectable, and Growth signature immediately has exactly two ordinary inputs |
| J18 | Edit one inherited basis RuleSet, compare another basis, reset to default, detach again | Copy-on-write affects only the selected basis; reset visibly relinks; sharing always includes kernels and Growth together |
| J19 | In Growth read the signature, press E, click source, move cursor, select text, insert/delete across lines | Full `fn growth(self: Scalar, ...)->Rate` signature and target are central; cursor, selection, syntax and diagnostics remain visible and responsive |
| J20 | Enter a valid one-kernel program with let/if/else and adjust a pinned input | Precise RGBA curve updates live; axes/range/cursor are interpretable; no character-art plot |
| J21 | Make the program invalid, then repair it | Old plot is explicitly stale, diagnostics identify source span, and repaired source creates a fresh valid plot |
| J22 | Use the two-kernel RuleSet and vary both inputs | Central plot changes to a precise 2D heatmap with meaningful color and zero contour |
| J23 | Review Experiment and Apply through visible control | Request/revision/ack are correlated, draft becomes clean, server reports NVIDIA/CUDA, and authoritative metadata matches basis/channel/RuleSet/kernel/source/parameters/colors |
| J24 | Leave Workbench with W | Every Workbench Kitty placement is deleted; simulation is fully visible with no stale overlay or cropped footer |
| J25 | Re-enter, resize Kitty smaller/larger, revisit all sections, paint, edit, Apply, leave | Layout adapts; footer never exceeds width; mouse mapping remains exact; no stale/cropped graphics |
| J26 | Disconnect/reconnect once and repeat pause, paint, Workbench entry and Apply | State and controls recover without duplicate server/process/image resources |

## Half-block fallback journey

Start a fresh released ARM64 C/S session with graphics forced off. Repeat J01,
J02, J03, J06, J07, J13–J17, J19–J25. Every control and keyboard/mouse path
must remain usable. Reduced image precision is allowed; wrong geometry,
unreachable cells, inert mouse actions, stale layers, or coordinate drift are
not.

## Direct Kitty SSH journey

Start a fresh Kitty window with:

```sh
kitten ssh tinker /home/wkj/.local/bin/cellarium
```

Visually verify launch, pause/resume, paint/erase, pointer mapping, Workbench
entry, Tiling/Kernel/Growth graphics, pan/zoom, exact numeric edit, source edit,
Apply, W return, resize, and exit. High-resolution graphics must remain usable;
direct-mode performance is recorded separately from C/S.

## Sustained mixed-use journey

For at least ten minutes, adaptively mix section clicks, basis selection,
middle-pan, zoom, polygon edits, kernel wheel/exact edits, source edits,
undo/redo, Apply/Revert, W transitions, resize, and one reconnect. Capture at
least once per minute and whenever behavior looks suspicious. Fail on any
frozen input, growing latency, coordinate drift, stale/cropped layer, lost
selection, incorrect authoritative state, or unrecoverable error.

## Cleanup and pass record

Stop only the recorded process group and server PID. Record:

- zero matching client/server child processes;
- no remaining private X socket/runtime directory;
- no growth in Kitty image placements or Cellarium shared-memory objects;
- final metrics for server simulation, snapshot receive, UI draw, fresh RGBA,
  Kitty presentation, input-to-ack, and input-to-visible;
- one evidence row with before/after PNG paths and the Agent's visual judgment
  for every required action.

Only a complete report with no unresolved defect is `pass`.
