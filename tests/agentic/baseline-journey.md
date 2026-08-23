# Current-release visual baseline journey

Run this journey against the latest published ARM64 release before changing
product code. Known defects are expected; retain them honestly.

## Mandatory route

For every row: capture before, visually locate, act with real X11 input, capture
after, visually judge, and add an observation. Never infer success from a trace.

| ID | User intent | Real interaction | Visible acceptance question |
|---|---|---|---|
| launch | Start remote simulation | Launch released `cellarium connect tinker` | Is a complete, unclipped Simulation visible and responsive? |
| discover | Find the editor | Read visible help, then use the displayed Workbench entry | Could a new user discover it without hidden knowledge? |
| sections | Understand navigation | Click every visible left-outline section once | Does selection move, does the center editor change, and is the inspector contextual? |
| tiling-select | Select a basis polygon | Click the central basis polygon | Is the selected polygon strong and one true topological neighbor ring ghosted? |
| tiling-draw | Draw/edit geometry | Drag vertices/edges and free-draw a polygon | Is input continuous, responsive, seam-aware, and visibly precise? |
| tiling-pan | Inspect a nonrectangular neighborhood | Middle-drag and wheel on empty canvas | Do camera motion and lattice geometry remain coherent? |
| kernel-select | Find one numeric unit | Click a source-basis/kernel cell | Are source basis, source channel, output target, extent, and values visible? |
| kernel-float | Make coarse and fine edits | Wheel, Shift-wheel, Ctrl-wheel; double-click or press Enter/E and type a float | Does each gesture edit the intended cell with precise feedback? |
| growth-edit | Understand the function | Click the central source editor; move cursor, select text, and type a body | Are full generated signature, target, kernel inputs, cursor, selection, syntax, and diagnostics visible? |
| growth-plot | Interpret the function | Enter valid then invalid source while moving pinned inputs | Is the RGBA plot precise, live on valid input, and clearly stale on invalid input? |
| metadata | Add explicit dimensions | Add one channel and one kernel through visible controls | Do defaults stay one channel/one kernel, and are new metadata/rules explicit? |
| apply | Commit the experiment | Use the visible Apply control | Is acknowledgement visible, draft state resolved, and remote state authoritative? |
| return | Return to simulation | Use the visible leave/Simulation control | Is Workbench graphics fully cleared with no stale Kitty placement? |
| stress | Repeat normal work | Switch all sections, edit, pan, undo/redo, Apply, and return repeatedly | Does input remain responsive and recoverable with no frozen state? |

## Required evidence

- Exact release tag, asset URL, SHA256, version, architecture, and mode.
- Before/after full-screen PNGs and a visual observation for every action.
- Reproduction steps and severity for every defect.
- Input-to-ack, input-to-visible, server sim, snapshot receive, UI draw, fresh
  graphics, and Kitty presentation cadence when available; metrics do not
  replace visual judgment.
- Identity-safe cleanup proof for both the local session and tinker server.
