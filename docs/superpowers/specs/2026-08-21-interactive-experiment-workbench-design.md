# Interactive Experiment Workbench Design

## Status

Approved conversational design, pending implementation-plan review.

## Purpose

Cellarium currently exposes a high-resolution simulation viewport and a compact
read-only editor panel, but it does not provide a discoverable way to design a
world, periodic tiling, channels, kernels, or growth behavior. Its bottom status
line also concatenates more text than the terminal can display.

The new workbench must make complete experiments editable inside the terminal
while preserving the existing direct simulation mode and the C1 client/server
mode. Editing is transactional: previews react immediately, but the running
simulation changes only after an explicit Apply.

## Goals

1. Provide a keyboard-and-mouse workbench for domains, periodic polygon
   tilings, channels, kernels, and growth programs.
2. Support a default single-channel experiment and true multi-channel,
   multi-kernel rules without changing conceptual models later.
3. Let users construct mixed periodic tilings such as regular octagons and
   squares, validate them reliably, and compile them to the existing sparse
   topology execution model.
4. Make growth programs readable through a small safe programming language and
   explain their behavior through live plots and sampled runtime data.
5. Keep the simulation responsive over SSH and avoid shifting performance work
   onto the weak local ARM64 client.
6. Preserve existing experiment loading, direct rendering, Kitty graphics, CPU
   fallback, and CUDA execution.

## Non-goals for the first release

- Curved tile edges or polygons with holes.
- Aperiodic and quasiperiodic tilings.
- An authoritative algorithm that always discovers the smallest fundamental
  period. The editor may suggest periods, but the user confirms two translation
  vectors.
- An unrestricted scripting runtime, loops, recursion, dynamic allocation, or
  external function calls in growth programs.
- Silent conversion of invalid drafts into a different valid experiment.

## Terminology

- **Tile**: one simulated spatial cell represented by a polygonal face.
- **Tile prototype**: a reusable regular or custom polygon definition.
- **Tile instance**: a rigidly translated and rotated prototype in a periodic
  pattern.
- **Fundamental patch**: the finite set of representative tile instances under
  the two user-confirmed translations. It replaces the earlier assumption that
  the user directly edits a parallelogram-shaped unit cell.
- **Domain**: the finite selection of repeated tiles that participates in a
  simulation.
- **Channel**: one scalar state field stored on every active tile. A channel is
  not a lattice site or a tile type.
- **Kernel slot**: a named convolution from one source channel to one target
  channel.
- **Growth program**: the scalar program that produces the growth rate or next
  state for one target channel.

## Application shell

Cellarium has two top-level modes:

- **Simulation** retains the current large viewport and optional inspector.
- **Workbench** uses the selected outline-first layout: a persistent experiment
  outline on the left, a large contextual editor canvas in the center, and a
  property inspector on the right.

The outline is stable across editors:

```text
Experiment
|- World
|- Tiling
|- Channels
|  |- A
|  `- ...
|- Kernels
|  |- inner: A -> A
|  `- ...
`- Growth
   |- A
   `- ...
```

`Tab` and `Shift+Tab` move between the outline, canvas, and inspector. Mouse
selection and direct manipulation have keyboard equivalents. `?` opens help
for the current focus. At narrow widths the inspector becomes a full-screen
panel instead of squeezing the canvas below a usable size.

### Footer

The footer has a fixed height of two terminal rows and never writes a string
longer than its allocated region:

1. mode, running/paused state, tick, current selection, and draft state;
2. the small set of commands relevant to the current focus plus `[?] Help`.

Detailed rates move to Statistics/Metrics. Segments have explicit priorities
and disappear as complete segments at narrow widths; text is never cropped in
the middle of a glyph.

## Transactional draft model

The active simulation owns an immutable `ActiveExperiment` and monotonic
`revision`. The workbench owns an independently mutable `ExperimentDraft` with
its `base_revision`.

Edits update local visual previews and lightweight diagnostics immediately.
They never mutate the active world. `Ctrl+Enter` submits the complete draft for
authoritative validation and compilation. A successful build atomically swaps
the experiment and increments the revision. A failure preserves both the old
running simulation and the user's draft.

Draft states are explicit: `APPLIED`, `MODIFIED`, `VALIDATING`, and `ERROR`.
Undo/Redo operates on semantic editor commands rather than raw terminal events.

## Experiment draft schema

The persisted and wire schema uses stable IDs rather than vector positions for
cross-references:

```text
ExperimentDraft
  metadata
  world
  tiling
  channels[]
  kernels[]
  growth_programs[]
  simulation_dt
  seed
  base_revision
```

### Channels

An experiment contains at least one channel. A newly created experiment has
exactly one channel. Logical state is `state[channel_id][tile_id]`; the compiled
representation remains channel-major for contiguous CPU/CUDA access.

Each channel stores a stable ID, name, optional frozen flag, initial field, and
display settings. Display color is presentation metadata and never enters the
simulation calculation.

A frozen channel may remain a kernel source but has no growth program. A kernel
targeting a frozen channel is a validation error because no update consumes its
result. A non-frozen channel may validly have zero kernels; its generated
signature then contains only implicit `self` and parameters.

Automatic display colors depend on visible channel count:

- one channel: near-white scalar values on black;
- two channels: a high-contrast complementary pair;
- exactly three channels: red, green, and blue;
- four or more: an accessible high-contrast palette that maximizes perceptual
  separation.

Manual color selection changes that channel to `Custom`; later automatic
palette changes do not overwrite it. Zero-valued pixels inside the domain are
pure black. The area outside the domain retains Cellarium's existing dark navy
background, so the domain remains visually bounded.

The viewport provides `Composite`, `Solo`, and `Grid` modes. Composite uses a
bounded screen-style blend. Hover/Inspect reports exact values for every
channel at a tile independently of display quantization.

### Kernels and channel routing

Every kernel slot has a stable ID, stable input symbol, display name,
`source_channel`, `target_channel`, definition, cutoff, and normalization.
Definitions may be explicit weights or formulas. Spatial tilings additionally
support distance/direction-derived weights; topological rules support graph
distance and explicit neighbor classes.

For each target channel, the generated growth signature contains exactly one
ordinary input for every kernel targeting that channel. `self` is always
available as a separate implicit input and does not count toward the kernel
input total. Parameters also do not count as inputs.

Reordering kernels does not rename symbols. Adding or removing a kernel updates
the signature immediately. References to removed symbols become draft errors;
new but unused inputs produce warnings rather than silent code rewriting.

## Periodic tiling model

The tiling editor operates on polygonal faces rather than forcing users to edit
basis sites and neighbor offsets.

```text
PeriodicTilingDraft
  translation_a: Vec2<f64>
  translation_b: Vec2<f64>
  prototypes[]
  instances[]
  simulation_mode: Topological | Geometric

TilePrototype
  id
  shape: RegularPolygon | SimplePolygon
  vertices
  presentation metadata

TileInstance
  id
  prototype_id
  rigid transform
```

Regular polygons retain their regularity constraint and side length. Custom
polygons are simple straight-sided polygons. Instances use rigid transforms;
prototype editing changes all attached instances, while Detach creates a new
prototype.

The user first lays out a small repeated pattern, then confirms two equivalent
translations. The system folds representatives into a fundamental patch and
shows surrounding periodic copies. Automatic period inference is advisory.

### Tiling interactions

- Drag regular or custom polygons from a palette.
- Translate and rotate instances.
- Snap a complete edge to a compatible edge; a successful snap makes both
  faces reference the same canonical edge instead of comparing approximate
  endpoint coordinates later.
- Use Fill Gap for non-authoritative shape suggestions.
- Toggle prototype, repetition, seam, adjacency, and expanded-domain overlays.
- Paint the finite domain on the expanded tile view.
- Use precise numeric fields for translations, rotations, side lengths, and
  vertices when mouse resolution is insufficient.

Regular triangle, square, hexagonal-cell, honeycomb, octagon-square `4.8.8`,
and other common tilings are presets made from the same editable data.

### Tiling compilation and validation

Editing geometry uses `f64`. Snapped topology uses canonical vertex and
half-edge IDs. Periodic half-edges carry explicit integer translation offsets.
Only compiled simulation values are converted to `f32`.

Errors that block Apply include:

- empty, non-finite, zero-area, or self-intersecting polygons;
- invalid regular-polygon constraints;
- degenerate or collinear translations;
- overlapping tile interiors;
- uncovered area inside the fundamental patch;
- an internal or periodic seam without exactly one opposite half-edge;
- incompatible paired-edge lengths or orientation;
- invalid references, unsafe compiled sizes, or inconsistent CSR arrays.

The authoritative coverage check clips representatives from a bounded set of
neighboring copies against the fundamental parallelogram. Their union must
cover it exactly once within a scale-aware tolerance. Edge pairing, area
coverage, and non-overlap are all required; vertex angle sums alone are not a
sufficient proof of tiling.

Warnings that permit Apply include isolated components, intentional self
loops, asymmetric directed neighborhoods, unusually long periodic offsets,
nearly degenerate translations, and multiple template edges that collapse to
the same finite periodic neighbor.

The inspector reports tile/prototype counts, expanded nodes and edges,
min/average/max degree, connected components, unmatched seams, overlap/gap
area, and CSR compilation status.

### Simulation meaning

`Topological` mode treats each tile as one equal-weight state node and derives
neighbors from shared half-edges. It is suitable for discrete and graph CA.

`Geometric` mode uses tile centers and areas. A spatial convolution is
discretized as an area-weighted sum and normalized according to its kernel
definition. This prevents mixed square/octagon tilings from implicitly treating
different physical areas as identical samples.

Apply compiles geometry into immutable arrays: centers, areas, render meshes,
adjacency CSR, and one sparse weight bank per kernel. No polygon clipping or
adjacency discovery runs during a simulation step.

## Workbench editors

### World

The World canvas edits finite rectangular, masked, or sparse selections over
the repeated tiling. Pencil, erase, rectangle, fill, and resize tools operate
on actual polygon tiles. The inspector selects Open, Constant, Periodic, Clamp,
or Reflect boundaries and previews how a selected edge resolves. Constant
boundary state is channel-specific, with a scalar broadcast convenience for
setting every channel at once.

### Tiling

The Tiling canvas is an unbounded real-space view with the polygon construction
and validation interactions above. It does not depict a rectangular `3x3`
board as the only possible cell shape. Translation-cell, polygon, graph,
periodic-repeat, and expanded-domain overlays are independent.

### Channel initial fields

Painting modifies the selected channel only while Composite updates live.
Users may switch channels, Solo one channel, or view a Grid. Fill, clear,
randomize, stamp, and import actions are draft operations and support Undo.

### Kernel

The Kernel canvas shows explicit weights as a heatmap/graph overlay or formula
weights as a spatial/graph-distance plot. Users can paint weights, masks, and
anchors where applicable; edit cutoff and normalization; enforce symmetry; and
preview which source tiles contribute to a selected target tile. Source and
target channel colors identify routing without relying on color alone.

### Experiment

The Experiment panel reviews metadata, all dirty sections, compatibility,
backend implications, validation diagnostics, load/save/export, and Apply or
Revert actions.

## Growth language and editor

Each non-frozen target channel owns one growth program. The editor displays its
generated read-only signature, for example:

```text
growth_B(inner, outer; self) -> rate
```

The body uses a small deterministic, statically typed scalar language:

```rust
let activation = gauss(inner, mu, sigma);
let inhibition = smoothstep(inhibit_low, inhibit_high, outer);

if self < capacity {
    2.0 * activation - inhibition
} else {
    -decay
}
```

The final expression is the result. First-release syntax includes immutable
`let` bindings, lexical blocks, `if/else` expressions, arithmetic, comparison,
booleans with short-circuit evaluation, line comments, parameters, kernel
inputs, `self`, constants, and a whitelisted scalar math library. It includes
the existing operations plus `log`, `tanh`, rounding/sign functions, `step`,
`smoothstep`, `mix`, `gauss`, `pi`, and `e`.

There is no assignment, mutable state, loop, recursion, user-defined function,
array, allocation, I/O, clock, or unseeded random source. AST depth and node
count are bounded. CPU interpretation and CUDA generation consume the same
typed AST and symbol table.

### Update semantics

Programs explicitly select one of two modes:

- `GrowthRate` (the Lenia default):
  `next = clamp(self + dt * result, 0, 1)`.
- `DirectUpdate` (generic/discrete rules):
  `next = clamp(result, 0, 1)`.

The complete equation is always visible above the source editor. The selected
mode is serialized and never inferred from the expression text.

### Live plots and traces

Parsing and lightweight evaluation are debounced and cancellable. Plot form
depends on the selected variables:

- one variable: curve and runtime histogram;
- two variables: heatmap, zero contour, and runtime density/scatter;
- more variables: two selected axes with sliders for all pinned inputs,
  parameters, and `self`.

Every immutable local binding is available as a plot target. Hovering a plot
shows kernel inputs, parameters, local values, selected branch, and final
result. Server-produced summaries describe the actual operating distribution
without sending every intermediate potential to the client.

Diagnostics carry source spans. The editor provides syntax highlighting,
bracket matching, completion, Undo/Redo, field-local messages, and function
help. Invalid source preserves the most recent valid plot as visibly stale and
never modifies the active simulation.

Static validation checks names, types, arity, missing and unused symbols,
complexity limits, and provable invalid numeric domains. Potential numeric
hazards are warnings when interval analysis cannot prove failure. Apply also
compiles the selected runtime backend and executes a non-committing test step.
Cross-backend builds are required by parity tests, not by users whose machine
does not provide CUDA. A runtime non-finite flag discards the generated step
buffer instead of swapping corrupted state into the world.

## Client/server protocol

The next protocol version carries full workbench metadata and stable IDs.
Apply uses request/response messages conceptually equivalent to:

```text
ApplyDraft { request_id, base_revision, draft }
ApplyAccepted { request_id, revision, normalized_experiment }
ApplyRejected { request_id, diagnostics[] }
```

Diagnostics contain severity, stable object/field path, source span when
applicable, and a human-readable message. Revision mismatch is explicit and
never causes last-write-wins replacement.

Remote snapshots separate authoritative simulation metadata from visual data.
The client subscribes only to channels needed by Composite, Solo, Grid, and
Inspect. Display planes may be quantized and compressed because authoritative
floating-point state remains on the server. Inspect requests return exact
values. Subscription changes are latest-only and must not block input.

Direct mode invokes the same validation, compile, and atomic-swap service in
process. It does not maintain a second interpretation of experiment semantics.

## Performance architecture

The local client performs editor interaction, small geometry previews, DSL
plots, compositing, and terminal presentation. It does not run performance
benchmarks or full simulation workloads for C/S mode.

Polygon rasterization caches a `screen pixel -> tile_id` map while camera and
geometry are stable. Fresh simulation frames then perform state lookups and
color blending only. Camera or geometry changes rebuild the map in a
cancellable latest-only worker. Tile triangulation is compiled once per Apply.

The server owns full tiling validation, sparse-weight construction, simulation,
runtime sampling summaries, and CUDA compilation. Existing input
sequence/acknowledgement rules remain authoritative for end-to-end latency.

## Persistence and migration

The experiment and lattice formats require new versions. Loaders perform pure
in-memory migrations before validation:

- legacy single-channel worlds become one channel;
- the legacy single kernel becomes one kernel slot targeting that channel;
- `potential` remains the migrated stable symbol for classic Lenia;
- legacy lattice sites become a topological tiling representation when no
  polygon geometry is available;
- missing display settings use the automatic palette.

New experiments use one channel, a one-tile square periodic tiling, and a
rectangular domain. This preserves the immediately runnable single-channel
experience while every field is represented by the new general model.

Newer unknown versions remain hard errors. Saving never silently drops tiling,
channel, kernel, or growth-program information.

## Error recovery

- Apply rejection leaves the active experiment and tick stream untouched.
- Disconnect preserves the local draft and its base revision.
- Reconnect with a conflicting revision offers reload or Save As; it never
  silently overwrites either side.
- Backend allocation or compilation failure is attached to the responsible
  draft field when possible and otherwise to the Apply operation.
- Revert restores the last authoritative experiment; Undo/Redo remains local to
  the current draft history.

## Verification strategy

### Unit and property tests

- Growth lexer/parser/type checker, source spans, short-circuit behavior,
  numeric failure handling, CPU interpreter, and CUDA code generation parity.
- Kernel-to-growth input cardinality and stable-ID behavior across add, delete,
  rename, reorder, and channel routing changes.
- Polygon simplicity, canonical snapped edges, half-edge pairing, periodic
  offsets, coverage, overlap, gaps, and scale-aware tolerances.
- Known valid square, triangular, hexagonal-cell, honeycomb, and octagon-square
  fixtures plus deliberately invalid seams and overlaps.
- Tiling-to-CSR compilation, topological/geometric weights, CPU/CUDA parity,
  and multi-channel/multi-kernel update parity.
- Persistence round trips and every supported legacy migration.

### UI tests

- Focus traversal and keyboard equivalents for mouse actions.
- Semantic Undo/Redo and transactional Apply/Revert.
- Growth editing, completion, diagnostics, plot selection, and trace values.
- Polygon placement, rotation, snap, seam highlighting, and domain painting.
- Composite/Solo/Grid colors, pure-black in-domain zero, dark-navy exterior,
  RGB defaults for three channels, and Custom color persistence.
- Footer behavior across supported terminal widths with no partial glyphs.

### End-to-end tests

- Direct and C/S modes load and Apply the same fixtures and produce identical
  normalized experiment metadata.
- PTY tests inject keyboard and mouse edits, require server input acknowledgement,
  Apply a valid draft, reject an invalid draft, and observe the corresponding
  consumed Kitty frame.
- Multi-channel display subscriptions and exact Inspect values are checked
  independently of optimistic client state.
- Performance and latency gates run on tinker's NVIDIA backend. The local ARM64
  client performs protocol, terminal, and low-cost presentation work only; its
  simulation or geometry throughput is not reported as product performance.

## Delivery sequence

Implementation should preserve a working product at each boundary:

1. responsive shell, two-line footer, Workbench navigation, and draft state;
2. versioned multi-channel/multi-kernel experiment model and atomic Apply;
3. growth language, editor, plots, and per-channel execution;
4. periodic polygon model, validation, presets, and topology compilation;
5. tiling/domain/kernel visual editors and cached polygon rendering;
6. remote visual subscriptions, exact Inspect, recovery, and full hybrid E2E
   coverage.

Each stage retains legacy direct rendering and existing Kitty graphics. No
stage makes an unvalidated draft the active simulation.
