# Heuristic seam assistant and workbench visual design

Date: 2026-08-29

## Why

Two complaints, both about the interface rather than the simulation:

1. The workbench is ugly.
2. The tiling assistant does not assist. It answers only about drawings that
   are already correct, and when it has nothing to say it says nothing.

## The seam assistant does not match its own specification

`2026-08-25-stable-workbench-geometry-design.md` already describes the intended
behaviour:

> Users may draw **approximate** polygons and place approximate neighbors. The
> assistant proposes candidate full-edge pairings using **endpoint distance,
> opposite direction, similar length, and periodic-offset consistency**.

`propose_full_edge_seams` implements none of that ranking. It reduces a pair to
one number — the maximum of three distances — and compares it against a fixed
`1e-3`. A pair either clears the bar and is proposed, or it does not exist.

Measured, by moving a single vertex of a preset:

| drawn accuracy | square | hexagon | triangles |
| --- | --- | --- | --- |
| exact | 2 seams | 3 | 3 |
| off by 1e-4 | 2 | 3 | 3 |
| off by 1e-3 | **0** | 1 | 1 |
| off by 1e-1 | 0 | 1 | 1 |

One thousandth of a unit side is finer than a mouse can be aimed, and at that
error the square loses every seam it had. What the user then sees is
`0 full-edge pairs proposed, worst residual 0.00e0` beside an Accept button
that does nothing, and a solve that refuses with `select at least one complete
edge pair`. The hexagon fails more quietly still: it keeps one seam of three
and never mentions the two it dropped.

None of this is for lack of knowledge. The same draft, handed to
`validate_coverage`, yields `the tiles cover 99% of the unit cell, leaving 1%
bare`. The program knows what is wrong and declines to say it.

A second refusal compounds this. With seams held, a vertex drag that the solver
cannot satisfy is rejected outright and the vertex does not move, under the
message `the held seams cannot follow this drag; try a smaller move or cancel
the seams`. Exploration is exactly what this forbids.

## What the assistant becomes

### A report that is never empty

`assess_seams` replaces the pass/fail proposal. Every ordered pair of boundary
edges is scored on the four signals the earlier spec names, and every boundary
edge ends up in the report under one of four headings:

- **held** — the pair already closes within solve tolerance.
- **ready** — the pair closes once accepted; the move needed is small enough to
  be a drawing inaccuracy rather than a different intent.
- **near** — a plausible partner needing a deliberate move. Offered with its
  gap, not hidden.
- **orphan** — no candidate at all, with the reason: no oppositely directed
  edge, no edge of comparable length, or no consistent periodic offset. The
  nearest rejected candidate is named so the reason is checkable.

Scores are reported, not just used, so a ranking can be argued with. Greedy
one-edge-one-partner matching is kept; it is the part that works.

### Hints carry a direction

Each unclosed candidate carries the translation that would close it, per
endpoint, in world units. The tiling canvas draws it: an arrow from the edge as
drawn to where it must go, coloured by bucket. This is the "which direction"
the interface currently withholds, shown rather than described.

Hints are live. They recompute as the outline is drawn and dragged, with no
button press; `Solve seams` stops meaning "go and find out" and starts meaning
"apply what is already on screen".

### Drags are never blocked

A drag that held seams cannot follow applies anyway. The seams that no longer
close are marked with their gap and direction and can be re-solved or released
individually. The solver keeps its current behaviour where it succeeds — the
whole equivalence class still moves together — and gives up its veto where it
fails.

## The visual design

`theme.rs` sets domain colours and nothing else: no `Visuals`, no fonts, no
spacing, no rounding. Every control is therefore stock egui dark, which is why
the workbench reads as a debugging tool.

The specific faults, none of which are matters of taste:

- Eleven identical grey rectangles in the window toolbar, in which `Apply &
  Run` — the primary action of the application — is indistinguishable from
  `Save as`, and destructive `Reset` carries the same weight as `Run`.
- `Run`, `Step` and `Reset` appear twice, once in the window toolbar and once
  in the Simulation toolbar, with identical labels and identical appearance.
  The duplication is load-bearing enough that `shell_harness` in
  `tests/gui_shell.rs` starts on the Tiling section specifically to avoid
  addressing two controls by one name.
- Labels follow their controls — `100.0% strength`, `1.000 value` — so each
  reads as the tail of the previous field.
- `Brush` names both a group and one of the tools inside it.
- One size and one weight throughout: section headings carry no more weight
  than the items beneath them.
- Numerals are proportional, so a changing readout jitters.
- The Properties inspector holds three lines above roughly nine hundred pixels
  of empty panel.

The canvases are not at fault and are not touched.

### What changes

A real `theme::install` establishing visuals, a type scale, tabular numerals
for readouts, spacing and rounding; and semantic constructors —
`primary`, `secondary`, `danger`, `section_header` — so weight follows meaning
instead of being chosen per call site.

Then the layout faults above, each fixed where it occurs: one home for
`Run`/`Step`/`Reset`, labels before their controls, the tool group renamed off
its member, and a Properties panel carrying the selected workspace's actual
state.

Layout structure is otherwise preserved, and the existing accessibility-driven
tests keep passing except where they encode a fault being fixed.

## Out of scope

Icons, a redesigned left rail, collapsible inspector groups, and any change to
the simulation, its backends, or the document model.

## What driving the real window changed

Four things were only visible once the built binary was operated on an X
display. They are recorded because in each case the code was defensible and the
screen was not.

**Two verdicts that looked like a contradiction.** Seam closure and plane
coverage were separate chips. A cell dragged into a strong shear reached a
state where every seam pair met at its endpoints while the tiles crossed their
own periodic copies — the program's own diagnostic was `edge 1 of tile 1
crosses through edge 2 of tile 1 in the copy -1 across and 0 up`. Both chips
were correct, and side by side they read as green "every seam closes" against
red "does not tile". They are now one sentence: *every seam meets, but the
tiles still overlap their own copies*. Closing a seam is about endpoints
meeting; tiling is about interiors staying out of each other's way, and the
interface has to say which one failed.

**A success painted as a failure.** `close_seams` reported through
`set_notice`, which the status bar draws in the problem colour, so the feature
announced itself in red every time it worked. It reports through `set_info`.

**A hint too small to see.** An arrow whose length is the gap is a few pixels
long for a gap of a hundredth of a unit — which is the common case, and the
case the whole feature exists for. The unclosed edge is now stroked in its
bucket's colour as well, so *which* edge is legible even when *how far* can
only be hinted at.

**A control disabled exactly when it was wanted.** `Close seams` was gated on
something being out of true, which left an already-exact tiling with no way to
hold its seams for linked dragging — the one thing a correct tiling needs the
control for. It is enabled whenever there is anything to act on.

Two more faults were found and fixed in the visual pass, both introduced by
this work: a Properties row whose value grew leftwards until it was drawn on
top of its own label (`CUDA (NVIDIA GeForce RTX 2080 Ti)`), and two separate
places where a new inspector label duplicated an existing control's name —
`Backend`, and every channel name. The second is the same "two nodes, one
label" fault this pass set out to remove from the toolbars, reintroduced in the
panel; the channel list became a summary, which is the better panel anyway.

One thing that looked like a bug was not. The status bar appeared clipped in
the first captures; the cause was the harness resizing the window immediately
after launch, before the application had settled its own size. Launching at the
natural size and leaving it alone renders correctly.
