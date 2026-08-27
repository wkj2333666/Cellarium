# Agentic GUI acceptance

This is a real user-session gate, not a substitute for the unit and integration
suites. An agent looks at the window, chooses coordinates from what it can
actually see, sends real X11 pointer and keyboard events, captures the result,
and records a judgement for every action.

It exists because the defects that matter most in a graphical application are
the ones a headless test cannot see. Every one of the following passed its unit
tests and was still wrong on screen:

- a glyph that rendered as a missing-character box, because the accessibility
  tree sees strings and not rasterized glyphs;
- a Run button that froze the display, because the repaint was scheduled from a
  stale snapshot;
- a randomize that filled the world with white, because the test asserted the
  values were in range and 1.0 is in range;
- two cell states drawn in colours that differed by a few units, which is two
  values and one colour to a person;
- a readout clipped off the bottom edge, because the canvas claimed the panel.

## Preconditions

Use a packaged release, not a development build: the thing being accepted is
what a user downloads.

```sh
./scripts/install-gui-local.sh cellarium-v<version>-<platform>-<arch>.tar.gz SHA256SUMS
```

Record, before starting: archive URL, its SHA256, `cellarium --version`, the
commit it was built from, the OS, the renderer in use, the compute probes the
Backend panel lists, and the screen geometry.

Start a clean session. Isolated `XDG_DATA_HOME` and `XDG_CONFIG_HOME`, one
Cellarium process, and no orphan left from a previous run — otherwise a pass may
be reading settings or an autosave that the run did not create.

## The journey

Every row is performed with the pointer. A row that needs the keyboard for
anything but typing text is a failure of the row, not of the tester.

1. **Simulation** — run, pause, step, reset, randomize, clear, fit. Paint into
   the world with the left button and erase with the right. Confirm the cell
   that changes is the cell under the cursor, at more than one zoom.
2. **Tiling from scratch** — draw a triangle, undo a point, redo it, close the
   polygon. Confirm the periodic copies appear and that the coverage verdict
   says whether it tiles.
3. **Tiling geometry** — apply the hexagon preset and confirm the neighbours
   meet with no gaps and no overlaps.
4. **Seams** — apply a preset with two polygons, solve the seams, read the
   residual, cancel, solve again, accept. Drag a vertex and confirm the held
   seams carry the equivalence class rather than tearing the tiling.
5. **Channels** — add two channels, confirm the palette, select each card, hide
   and show, freeze and thaw, recolour, delete, undo. Confirm the preview says
   whether it is showing the live world or draft initial values.
6. **Kernels** — add four, select them out of creation order, give each a
   distinct value, and confirm each keeps its own. Switch cells in and out of
   the support. Type an exact value.
7. **Referenced deletion** — make the growth program read a kernel, delete that
   kernel, read the proposed rewrite, cancel, confirm nothing changed, then
   repeat and accept.
8. **Growth** — add kernels and watch the signature widen. Type invalid source
   and confirm it is kept, marked in place, and stops the plot. Fix it. Change
   the axes, pin an input, switch Rate and Value.
9. **Apply & Run** — apply an invalid draft and confirm the running world is
   untouched; fix it and confirm the new one runs.
10. **Backends** — switch between Auto and CPU and confirm the status bar names
    what is actually running.
11. **Persistence** — save, close, reopen, and confirm the experiment came back.
12. **Stress** — resize the window repeatedly and interact at several sizes.
    Confirm coordinates stay correct after every resize.

## Judging

For every row, answer four questions. Any "works but unclear" is a failure.

- Could a user who has not seen this before find the next action?
- Did the visual result match what the action promised?
- Did any stale, blank, torn or overlapping frame appear?
- When something was refused, was the reason understandable and actionable?

The last one is the most commonly failed. An operation that silently does
nothing is indistinguishable from an application that has stopped responding.

## Evidence

Write to `target/agentic-gui/<candidate-sha>/`:

- `manifest.json` — archive URL, checksum, version, commit, OS, renderer.
- `environment.txt` — probes, screen geometry, session type.
- `actions.jsonl` — one line per action: what was clicked, where, and why.
- `before/` and `after/` — a PNG for every row.
- `observations.md` — the four judgements per row.
- `result.txt` — PASS or FAIL, and for a FAIL what blocked it.

Never edit the evidence of an older candidate to describe a newer one. Each
candidate gets its own directory.

## When a row fails

Reproduce it with a focused deterministic test first, then fix it, then build a
new candidate and repeat the affected row along with its neighbours. A fix
without a test that would have caught it is a fix that will be made again.

## Final audit

After exiting: no Cellarium process, no session the test started, no child
process, no temporary candidate and no locked workspace may remain. Keep only
the evidence directory.
