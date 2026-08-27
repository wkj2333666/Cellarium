# Cellarium Feature and Acceptance Inventory

> **Migration note (2026-08-27):** The body of this document records historical
> feature facts for the v0.2.2 TUI. The next major version will migrate to a
> local egui/wgpu GUI and remove the TUI, Kitty/half-block rendering, `server`,
> `connect`, SSH, and the remote protocol. Sections 12 and 13, together with
> acceptance requirements that depend on remote client/server operation, are
> superseded by `docs/gui-migration-handoff.md` and the GUI migration
> specification. The data model, editing features, and experiment semantics
> remain the feature-parity inventory for the migration.

> Reference version: v0.2.2 (worktree commit `271b79e`)
> Document purpose: product feature inventory, status checklist, and defect list.
> This document marks a capability as implemented only when it exists in code;
> designs and verbal agreements must not be presented as completed work.

## 1. Status legend

| Mark | Meaning |
| --- | --- |
| ✅ | Implemented, with automated-test or actual-interaction evidence |
| 🧪 | Implemented, but still missing complete real user-level agentic acceptance |
| ⚠️ | Implemented with a reproduced defect; not accepted |
| ⏳ | Confirmed requirement, but not fully implemented or verified |
| ❌ | Explicitly unsupported, cancelled, or obsolete legacy requirement |

## 2. Product role and launch modes

### 2.1 Single executable

- ✅ One `cellarium` binary currently provides direct local operation, the
  remote server, and the client/server client.
- ✅ `cellarium`: simulate and render directly on the current device.
- ✅ `cellarium server`: run the remote protocol server over stdin/stdout.
- ✅ `cellarium connect <ssh-host>`: start the remote server through SSH and
  render and interact locally.
- ✅ `--ssh-command <path>`: select the SSH executable; also configurable with
  `CELLARIUM_SSH_COMMAND`.
- ✅ `--kernel <path>`: load a kernel definition directly.
- ✅ `--experiment <path>`: load an experiment directly.
- ✅ `--save-experiment <path>`: write the current experiment.
- ✅ `--version` / `-V`.
- ✅ Reject incompatible protocol versions instead of silently decoding them
  incorrectly.

### 2.2 Platforms and compute backends

- ✅ Linux x86_64 and ARM64 artifacts.
- ✅ macOS x86_64 and ARM64 artifacts.
- ✅ Windows x86_64 and ARM64 artifacts.
- ✅ Linux binaries detect CUDA dynamically and use an available NVIDIA/CUDA
  GPU.
- ✅ Automatically fall back to CPU when CUDA is unavailable.
- ✅ macOS and Windows currently use the CPU backend.
- ✅ Built-in Conway and Lenia/Orbium rules can be selected from the simulation
  screen.
- 🧪 Custom-basis, multi-channel, and multi-kernel experiments support both CPU
  and CUDA. Every stable release still requires numerical-consistency and real
  runtime acceptance on both paths.

## 3. Core data model

### 3.1 Terms

| Term | Meaning |
| --- | --- |
| World | Current simulation state containing every channel value for every logical cell |
| Tiling | Periodic tiling geometry: translation basis vectors, prototype polygons, central-cell instances, and seams |
| Basis polygon | A polygon with independent state semantics inside one unit cell; it is not a color channel |
| Channel | One scalar state layer on every basis polygon |
| Binding | One `(basis polygon, output channel)` pair |
| RuleSet | The complete rule for a Binding: kernels, Growth program, parameters, and update mode |
| Kernel | Samples and convolves a source channel to produce one scalar Growth input |
| Growth | Computes the next Value or Rate from `self`, every kernel input, and parameters |

### 3.2 Cardinality rules

Let:

- `B` be the number of basis polygons in the central unit cell;
- `C_active` be the number of non-frozen channels;
- `K(b,c)` be the kernel count for Binding `(basis=b, channel=c)`.

Then:

1. ✅ The logical number of Growth/RuleSet bindings is `B × C_active`.
2. ✅ Every Binding has its own Growth program.
3. ✅ Every Binding has one kernel by default; more kernels require explicit
   user action.
4. ✅ A Growth program has `K(b,c)` kernel parameters, not the global channel
   count.
5. ✅ Its total argument count is `1 + K(b,c)`: one `self` plus one scalar
   per kernel.
6. ✅ A kernel's source channel may differ from its output channel, enabling
   cross-channel coupling.
7. ✅ Multiple Bindings may initially share one RuleSet. Editing one detaches it
   through copy-on-write to reduce multi-basis and multi-channel editing effort.

Examples:

| Polygons in unit cell | Active channels | Growth Bindings | Default effective kernels |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 1 | 1 |
| 1 | 3 | 3 | 3 |
| 2 | 1 | 2 | 2 |
| 2 | 3 | 6 | 6 |

"Default effective kernels" assumes one kernel per Binding. Adding a second
kernel to any Binding increases the total by one.

### 3.3 Defaults

- ✅ The default experiment is single-channel Lenia/Orbium with a 256×256
  periodic world.
- ✅ Default channel count: 1.
- ✅ Default kernel count per Binding: 1.
- ✅ Adding a channel or kernel requires an explicit user action.
- ✅ Without a custom Tiling, simulation uses the compatible square RasterGrid.
- ✅ Selecting New blank in Tiling produces an empty canvas and does not
  silently create a square tiling.

## 4. Main simulation screen

### 4.1 Simulation controls

- ✅ Pause/resume: `Space` or `P`.
- ✅ Single step: `N` or `Enter`.
- ✅ Reset: `R`.
- ✅ Randomize: `A`.
- ✅ Clear: `C`.
- ✅ Select Conway: `1`.
- ✅ Select Lenia: `2`.
- ✅ Enter Workbench: `W`.
- ✅ Quit: `Q`, `Esc`, or `Ctrl+C`.

### 4.2 Viewport interaction

- ✅ Left-button paint with continuous dragging.
- ✅ Right-button erase with continuous dragging.
- ✅ Middle-button canvas pan.
- ✅ Wheel zoom centered at the pointer.
- ✅ Inspect the exact value of a cell with the pointer.
- ✅ Fit to available canvas while preserving pan and zoom state.
- ⚠️ These paths have historically suffered from coordinate offsets, zoom
  flicker, and an initially tiny image. Targeted fixes exist, but the risk
  remains open until a complete stable-release agentic regression passes.

### 4.3 Status and performance metrics

- ✅ Show current rule, running/paused state, tick, world size, zoom, inspect
  value, and display protocol.
- ✅ Direct mode shows backend-step and UI/render costs.
- ✅ Client/server mode distinguishes:
  - server simulation rate;
  - snapshot receive rate;
  - UI draw rate;
  - fresh RGBA graphics rate;
  - Kitty presentation/consume rate when observable;
  - input sequence and acknowledgement.
- ✅ Metrics use independent event sources instead of treating every UI redraw
  as a new graphics frame.

## 5. General Workbench interaction

### 5.1 Layout

- ✅ Left Experiment outline: World, Tiling, Channels, Kernels, Growth, and
  Experiment.
- ✅ Center Canvas: primary visualization and editor for the current section.
- ✅ Right Inspector: current object, state, shortcuts, diagnostics, and syntax
  help.
- ✅ Wide terminals show three columns; narrow terminals hide the Inspector and
  wrap the toolbar to at most four rows.
- ✅ Outline items and toolbar actions are pointer-clickable.
- ✅ `T` or a click changes section.
- ✅ `Tab` / `Shift+Tab` moves focus among Outline, Canvas, and Inspector.
- ✅ The Inspector scrolls vertically with the wheel.

### 5.2 Draft transactions

- ✅ Workbench stores both the authoritative applied experiment and the current
  draft.
- ✅ Status values: Clean, Dirty, Invalid.
- ✅ `Ctrl+Z` / `Ctrl+Y`: undo/redo.
- ✅ `Ctrl+R`: restore the draft from authoritative state.
- ✅ `Ctrl+Enter`: validate, Apply, and start running.
- ✅ An Invalid draft cannot Apply or overwrite the valid remote experiment.
- ✅ Client/server Apply carries a base revision; a remote conflict is never
  overwritten silently.
- ✅ The remote side returns authoritative experiment metadata so the client can
  mirror basis, channel, RuleSet, kernel, Growth, and editor state.
- ✅ `W` leaves Workbench for simulation; old Workbench graphics placements
  must be deleted.

## 6. World editor

- ✅ Display the current or draft world.
- ✅ `]` selects the editing channel.
- ✅ `V` switches between Composite and selected-channel views.
- ✅ Left paint, right erase, middle pan, and wheel zoom.
- ✅ World draft edits support undo and redo.
- 🧪 Custom polygon/basis simulation must render actual polygon geometry rather
  than reverting to squares. Keep this in every release's real visual journey.

## 7. Tiling and unit-cell editor

### 7.1 Creation entry points

- ✅ Start from blank with `B` / New blank.
- ✅ Presets:
  - Square;
  - Equilateral triangles, with two triangle bases per unit cell;
  - Regular hexagon;
  - Octagon + square, the 4.8.8 tiling with two bases per unit cell.
- ✅ `P` cycles presets.
- ✅ `D` enters the draw-shape tool.
- ✅ `A` adds a basis polygon.
- ✅ `N` selects the next basis.
- ✅ `+` / `-` changes regular-polygon side count from 3 to 64.
- ✅ `0` fits the tiling to the canvas.

### 7.2 Free drawing

- ✅ Draw a custom polygon point by point with the pointer.
- ✅ An open path shows a preview line to the pointer.
- ✅ Close a polygon by clicking its first vertex, double-clicking, or pressing
  `Enter`.
- ✅ `Esc` cancels the current construction.
- ✅ During construction, `Ctrl+Z` removes the most recently placed vertex and
  `Ctrl+Y` restores it.
- ✅ Reject immediately when placing a vertex that:
  - coincides with an existing vertex;
  - makes the new edge intersect or touch the existing open path;
  - has non-finite coordinates;
  - exceeds the 64-vertex limit.
- ✅ On closure validate at least three points, counter-clockwise orientation,
  nonzero area, no zero-length edge, and no self-intersection.
- 🧪 Closure once had a defect where documented actions did nothing. All three
  closure paths exist now, but every release must verify them with real pointer
  and keyboard input.

### 7.3 Periodic-unit-cell display and selection

- ✅ Emphasize editable basis polygons in the central unit cell.
- ✅ Render neighboring periodic copies at reduced opacity to communicate the
  real tiling instead of showing only an axis-aligned rectangular grid.
- ✅ A regular hexagon uses non-orthogonal translation vectors; Octagon + square
  shows mixed polygons.
- ✅ Clicking either the central representative or a periodic copy maps back to
  the corresponding basis.
- ✅ Select, drag vertices, right-click delete, wheel zoom, and middle-button pan.

### 7.4 Tiling assistance and constraints

- ✅ Allow only complete edge-to-edge seams.
- ❌ T-junctions are explicitly unsupported. Any legacy document that permits
  them is obsolete.
- ✅ Validate periodic coverage for gaps, overlaps, crossings, open seams,
  orientation/degeneracy problems, and Euler-topology consistency.
- ✅ `S` Solve seams: propose pairings among nearby complete edges and jointly
  optimize vertices and translation vectors into an exact periodic tiling.
- ✅ Preserve seam constraints after solving. Dragging one constrained vertex
  then moves related vertices and lattice vectors together to preserve tiling
  as far as possible.
- ✅ Display solved seam count, maximum displacement, residual, and diagnostics.
- ⚠️ The solver assists from sufficiently close complete edges; it is not a
  global combinatorial search over arbitrary sketches. If no complete edge pair
  can be found, ask the user to move corresponding edges closer first.
- 🧪 The complete "rough layout → remove gaps automatically → constrained fine
  tuning" flow has an algorithmic skeleton and unit tests, but still lacks full
  agentic acceptance for complex multi-polygon unit cells.

## 8. Channels editor

### 8.1 Channel management

- ✅ One `state` channel by default.
- ✅ `A` adds a channel.
- ✅ `Del` removes the selected channel.
- ✅ `]` selects the next channel.
- ✅ Every channel has its own clickable Inspector row.
- ✅ `F` freezes or thaws a channel. Frozen channels no longer own Growth
  Bindings that require updates.
- ✅ `X` shows or hides a channel.

### 8.2 Color and composition

- ✅ `V` switches between Composite and single-channel views.
- ✅ `C` cycles color presets.
- ✅ `E` enters an exact RGB color.
- ✅ One channel defaults to a high-contrast light color on black.
- ✅ Three channels default to RGB.
- ✅ The in-domain background is pure black.
- ✅ The out-of-domain region retains a dark background to distinguish the
  actual simulation domain.
- ✅ Color, visibility, and opacity are persistent experiment data.
- ✅ The Channels Canvas shows real running state rather than random placeholder
  noise.
- ✅ Custom non-rectangular tilings use the same polygon scene as Simulation
  rather than shearing a 256×256 raster.

### 8.3 Known lifecycle defects

- ⚠️ After deleting and then adding a channel,
  `WorkbenchState::add_channel` may derive a duplicate name from the length,
  such as a second `channel_3`, making the draft Invalid.
- ⚠️ Undo after that sequence may leave `selected_channel` pointing to a
  missing channel, causing the Inspector to show `selected: —`.
- ⚠️ Freezing a channel in a normalized multi-basis rule can leave incomplete
  RuleSet/default/binding cleanup and references to the frozen channel, making
  the draft Invalid.
- ⚠️ Therefore the ordinary add/display path works, but
  "delete → add → undo" and freeze/thaw are not accepted.

## 9. Kernels editor

### 9.1 Ownership and routing

- ✅ A Kernel belongs to the selected `(basis, output channel)` RuleSet.
- ✅ One kernel exists by default; `A` explicitly adds more.
- ✅ `Del` removes the selected kernel.
- ✅ `]` selects the next kernel.
- ✅ `S` changes the source channel.
- ✅ `U` changes the output channel or Binding.
- ✅ Reject deletion when it would leave a missing Growth reference; never
  create a partially invalid state.
- ✅ RuleSet sharing, local override, copy-on-write detachment, and reset to
  default.

### 9.2 Visualization

- ✅ High-resolution graphics visualize both raster kernels and periodic
  polygon/basis kernels.
- ✅ One numeric unit in a periodic kernel is one basis polygon; a hexagonal
  kernel is not redrawn as squares.
- ✅ Visually distinguish active, zero, inactive/outside-support, and empty cells.
- ✅ Show the selected cell, source basis, offset, anchor, and numeric value in
  the Inspector.
- ✅ Large kernels support pan and zoom so every internal cell remains reachable.
- ✅ `0` fits the kernel to the canvas.

### 9.3 Values and support editing

- ✅ `M` switches between Weights and Support tools.
- ✅ Left-button drag paints weights; right button sets zero.
- ✅ After selecting an active cell, wheel changes its floating-point value:
  - normal step: ±0.05;
  - Shift: ±0.005;
  - Ctrl: ±0.5.
- ✅ Wheel over an inactive or empty position zooms and cannot change a value
  accidentally.
- ✅ `E` or `Enter` opens exact floating-point input with commit, cancel, and
  invalid-value diagnostics.
- ✅ `R` edits stencil dimensions and anchor.
- ✅ The Support tool controls kernel shape and activation mask rather than only
  changing values.

### 9.4 Presets and sampling geometry

- ✅ `P` generates Gaussian weights over the current support.
- ✅ `G` edits Gaussian sigma exactly.
- ✅ `Q` switches between two sampling metrics:
  - **Affine / LatticeAffine** samples in lattice coordinates and deforms with
    the lattice affine transform;
  - **World / WorldEuclidean** samples by the real polygon positions in world
    space, preserving an intuitive circular/Gaussian shape on hexagonal and
    other non-orthogonal tilings.
- ✅ Potential remains the raw weighted convolution sum and is not divided by
  the total kernel weight.
- ⚠️ The Kernel page has historically suffered from unselectable hexagons,
  misleading outer-ring colors, and an entirely black empty kernel. Shared
  coordinate transforms, inactive locking, and empty-state messages now exist,
  but complete agentic regression is still required.

## 10. Growth editor

### 10.1 Binding and signature

- ✅ The target is shown explicitly as `basis B / channel C`.
- ✅ The full signature appears in both Canvas and Inspector:

  ```text
  fn growth(self: Scalar, k1: Scalar, ..., kN: Scalar) -> Rate|Value
  ```

- ✅ `self` is the current value of the target basis/channel.
- ✅ Each `kN` is the raw convolution result of the corresponding Kernel in
  that RuleSet.
- ✅ Parameters such as `mu` and `sigma` are external read-only scalars.
- ✅ The signature and input count update when the kernel count changes.

### 10.2 Update modes

- ✅ `M` switches between:
  - **Rate**: `next = clamp(self + dt × result, 0, 1)`;
  - **Value**: `next = clamp(result, 0, 1)`.
- ✅ Experiment edits `dt`.
- ✅ `clamp(x, lo, hi)` returns `lo` below `lo` and `hi` above `hi`.
- ✅ Potential is not automatically normalized before entering Growth.

### 10.3 Language

- ✅ Rust-like expression language, not complete Rust.
- ✅ The final expression without a semicolon is the block or program result.
- ✅ `let name = expression;`.
- ✅ `if condition { expression } else { expression }`; `else` is required.
- ✅ Numbers, `true`, `false`, `pi`, and `e`.
- ✅ Single-line `// comment`.
- ✅ Arithmetic: `+`, `-`, `*`, `/`, `^`, and `!`.
- ✅ Comparisons: `==`, `!=`, `<`, `<=`, `>`, and `>=`.
- ✅ Logical operators: `&&` and `||`.
- ✅ Built-ins:
  - `sqrt(x)`, `abs(x)`, `exp(x)`, `log(x)`;
  - `sin(x)`, `cos(x)`, `tanh(x)`;
  - `floor(x)`, `ceil(x)`, `round(x)`, `sign(x)`;
  - `min(a,b)`, `max(a,b)`, `step(edge,x)`;
  - `clamp(x,lo,hi)`, `smoothstep(lo,hi,x)`;
  - `mix(a,b,t)`, `gauss(x,mu,sigma)`.
- ❌ The language currently has no `return`, loops, mutable variables, or side
  effects. A branch value is the branch's final expression.
- ✅ Diagnostics cover type errors, unknown variables/functions, argument
  counts, condition types, and result types.
- ✅ Analyze dangerous numeric ranges, including possible non-finite values.

### 10.4 Text-editing experience

- ✅ `E` starts or finishes source editing; `Esc` finishes.
- ✅ Multi-line editing, visible caret, and selection highlight.
- ✅ Arrow keys, Home/End, and word movement.
- ✅ Backspace/Delete and newline.
- ✅ Shift extends selection.
- ✅ `Ctrl+A` selects all; `Ctrl+U` deletes to line start.
- ✅ Every edit reparses, type-checks, and refreshes diagnostics live.
- ✅ The right Inspector provides scrollable syntax, built-ins, signature, mode,
  variable meanings, and parameter help.

### 10.5 High-resolution graphics plot

- ✅ With zero or one kernel input, render a high-resolution pixel curve.
- ✅ With two or more kernel inputs, render a 2D heatmap over the first two while
  holding the rest fixed.
- ✅ Show axes, ranges, curve/color output, and zero reference.
- ✅ `d` / `D` edits the plot minimum/maximum exactly.
- ✅ Derive the default plot domain from kernel weights and input ranges instead
  of fixing it to [0,1].
- ✅ An invalid program retains the last valid plot, marks it stale explicitly,
  and shows source-span diagnostics.
- ⚠️ Growth plots have historically appeared as flat lines or empty. Equality
  conditions such as `potential == 2/6` only hit exact sample points; the plot
  must show isolated threshold markers rather than silently appearing
  identically zero. This remains mandatory in stable-release agentic testing.

## 11. Experiment, Apply & Run, and persistence

### 11.1 Experiment inspection and execution

- ✅ Summarize world dimensions, basis count, channel count,
  RuleSet/Binding count, total effective kernels, Growth count, `dt`, seed,
  and diagnostics.
- ✅ `D` edits simulation `dt` exactly.
- ✅ `Ctrl+Enter` means **Apply & Run**, not merely save:
  1. validate the complete draft;
  2. compile topology, RuleSets, and Growth;
  3. send remote Apply in client/server mode or replace the local backend;
  4. clear paused state and start running;
  5. after receiving the new revision/acknowledgement, mark the draft Clean.
- ✅ Failed Apply leaves the original authoritative experiment runnable.

### 11.2 Default persistence

- ✅ Data directory:
  - if `XDG_DATA_HOME` is absolute: `$XDG_DATA_HOME/cellarium/`;
  - otherwise: `$HOME/.local/share/cellarium/`.
- ✅ `workbench.ron`: active, draft, active revision, and base revision.
- ✅ `experiment.ron`: a self-contained loadable and runnable experiment.
- ✅ `Ctrl+S` saves active/workspace state.
- ✅ `Ctrl+E` exports the draft.
- ✅ `Ctrl+L` loads a draft.
- ✅ Periodic automatic Workbench save.
- ✅ Atomic writes through temporary file, sync, and rename; new Unix files use
  mode 0600.
- ✅ RON files contain a format version; reject unknown newer versions.
- ✅ Legacy experiment formats have a controlled migration boundary and never
  silently reinterpret new fields.

## 12. Graphics, terminals, and fallback

> Historical for the TUI; removed by the GUI migration.

- ✅ Kitty graphics, Sixel, iTerm2 graphics, and half-block are supported.
- ✅ When Kitty or another supported graphics protocol is detected, graphics is
  the default.
- ✅ On local Unix Kitty, prefer shared-memory frame transfer and fall back to
  inline graphics.
- ✅ If all graphics protocols are unavailable, fall back to half-block.
- ✅ In client/server mode the local client performs high-resolution rendering;
  the server simulates and sends logical snapshots.
- ✅ A latest-frame-first queue drops obsolete intermediate frames under
  backlog, so input does not wait for old image encoding.
- ✅ Delete old Kitty placements on Workbench section change, resize, exit,
  protocol fallback, and leaving Workbench.
- ✅ Half-block and graphics share the same controller and logical coordinate
  transform, so fallback must preserve pointer and keyboard interaction.
- 🧪 Direct `kitten ssh` retains high-resolution graphics, but performance and
  interaction latency must be measured independently from client/server mode.

## 13. Remote client/server mode

> Historical for the TUI; removed by the GUI migration.

- ✅ An SSH subprocess carries the versioned binary protocol over stdin/stdout.
- ✅ The server performs GPU/CPU steps; the client owns terminal UI, graphics,
  and input.
- ✅ Latest-only snapshots prevent network jitter from accumulating stale state.
- ✅ Every input carries a sequence number and the server returns
  `applied_input_seq`, enabling true end-to-end input-acknowledgement
  measurement.
- ✅ Apply carries a revision; the remote side returns authoritative
  ExperimentSpec and selected editor metadata.
- ✅ Client-side optimistic feedback and server acknowledgement are measured
  separately.
- ✅ Disconnect, exit, and test cleanup must terminate only the current session
  and leave no extra server, Kitty image, or shared-memory object.

## 14. Test and release gates

### 14.1 Automated tests

- ✅ Rust unit tests cover the model, parser/type checker, kernels, tiling,
  solver, render transforms, history, and protocol.
- ✅ CPU and CUDA backend paths have tests.
- ✅ PTY E2E covers protocol, pointer/keyboard bytes, Apply acknowledgement,
  Kitty command consumption, and half-block.
- ✅ GitHub Actions builds one binary for multiple OS/architecture combinations
  and publishes `SHA256SUMS`.

### 14.2 Agentic user-level testing

- ✅ A real test harness exists: Xvfb → Openbox → Kitty → released ARM64 client
  → tinker server.
- ✅ The agent must inspect the real framebuffer, choose coordinates from the
  latest screenshot, send real X11 pointer/keyboard events, and judge the
  visual result.
- ✅ Every action requires before/after PNG evidence and a semantic observation;
  a static test or image hash alone cannot count as acceptance.
- ✅ Both Kitty and half-block must complete critical journeys.
- ✅ The local Raspberry Pi runs only prebuilt release clients; it never builds
  locally. GPU/performance work runs on tinker.
- ⚠️ The latest v0.2.2 Channel/Growth cardinality journey reproduced the
  defects in Section 8.3, so the current overall result is not PASS.

## 15. Known issues that still require fixes

Ordered by current priority:

1. ⚠️ **Deleting and then adding a channel can create a duplicate name and make
   the draft Invalid.**
2. ⚠️ **Undo after that sequence can leave the selected channel dangling.**
3. ⚠️ **Freezing a normalized multi-basis channel can leave incomplete
   RuleSet/binding cleanup.**
4. ⚠️ **Inspector count scopes are unclear.** It currently places global
   `channels: N` beside the current Binding's `kernels: K`, which suggests
   that the values should match. It should distinguish:
   - basis polygons;
   - active and frozen channels;
   - Growth Bindings = `B × C_active`;
   - current-Binding kernel count;
   - total effective kernel count across all Bindings.
5. 🧪 **The complete stable-release agentic regression has not passed again.**
   It must cover drawing and closing a triangle, true geometry after applying a
   hexagon, RGB Channels, Kernel support/floating-point/exact entry, Growth
   curve/heatmap, Apply & Run, resize, graphics cleanup on exit, and half-block.

## 16. Confirmed capabilities that still need product refinement

- ⏳ Stronger global tiling assistance: when polygons are far from a tileable
  arrangement, provide intuitive candidate edge correspondences and actionable
  repair suggestions instead of only reporting errors.
- ⏳ For complex multi-polygon cells, improve discoverability, conflict
  explanation, and failure recovery for "rough layout → solve → constrained
  vertex refinement."
- ⏳ Add direct visual controls for RuleSet sharing, local override, and reset
  to default so users do not infer state from Inspector text.
- ⏳ Make the legend and current tool for Kernel
  active/inactive/support/zero states more prominent.
- ⏳ When Growth uses more than two kernels, the plot currently uses the first
  two for a heatmap and pins the rest; provide a clearer pinned-input UI.

## 17. Cancelled or obsolete requirements

- ❌ Do not permit T-junctions. Legacy `tests/agentic/full-journey.md` J09 and
  early design documents that permit them must be updated.
- ❌ "Tiling starts with a square polygon" is obsolete. A new design starts
  blank and the user selects a preset or draws from scratch.
- ❌ Growth does not require explicit `return`; its final expression is the
  result, allowing `if/else` to produce branch values naturally.
- ❌ Potential is not automatically normalized; preserve the raw kernel
  convolution value.
- ❌ Do not force Channel count to equal Kernel count.
- ❌ Do not use character art as the production Kernel/Growth visualization.
  Graphics is primary; half-block only provides an interactive fallback.

## 18. User review checklist

Review the following categories for omissions:

- [ ] Launch modes and platforms
- [ ] Main simulation controls
- [ ] World editing
- [ ] Unit-cell/tiling creation, validation, solving, and constrained editing
- [ ] Basis-polygon and Channel semantics
- [ ] Channel cardinality, colors, visibility, and freezing
- [ ] RuleSet sharing and detachment policy
- [ ] Kernel cardinality, routing, shape, support, values, and presets
- [ ] Growth signature, syntax, Rate/Value, and high-resolution plots
- [ ] Apply & Run
- [ ] Save, autosave, load, and format migration
- [ ] Direct/client-server, Kitty, and half-block behavior
- [ ] Performance metrics, process cleanup, and test gates

If any product semantic differs from user intent, update this inventory before
changing implementation or agentic journeys. This prevents another false
"tests passed" result that validated the wrong product behavior.
