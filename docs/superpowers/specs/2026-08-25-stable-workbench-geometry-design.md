# Stable Workbench Geometry and Kernel Authoring Design

**Status:** Approved in conversation on 2026-08-25.

## Relationship to earlier designs

This specification is the final stable-release delta over
`2026-08-23-basis-aware-workbench-agentic-validation-design.md`. It
supersedes every earlier requirement that permits T-junctions, treats a
rectangular raster as the authoritative display for a periodic polygon
experiment, hides kernel masks or dimensions, samples all presets in lattice
coordinates, or requires public release-candidate tags.

The existing C/S protocol, direct mode, CUDA runtime, one-binary distribution,
Kitty graphics transport, half-block fallback, RuleSet inheritance, typed
growth language, and XDG persistence remain required.

## Stable-release scope

The stable release must close the following user-visible gaps together:

1. A custom periodic tiling is drawn, assisted, solved, validated, simulated,
   and rendered as its actual polygons rather than as square raster pixels.
2. Tiling seams are strictly edge-to-edge. T-junctions are invalid and are not
   automatically split into apparently valid topology.
3. A new custom tiling draft starts empty. Square, triangle, regular hexagon,
   honeycomb, and octagon-square are explicit presets, not hidden defaults.
4. Kernel support, dimensions, anchor, floating-point values, and presets are
   all directly editable and visually explained.
5. Kernel presets support both lattice-coordinate (affine) sampling and
   world-coordinate (shape-preserving) sampling.
6. Channels preview authoritative experiment state in the real tiling
   geometry; it never stretches the initial-value vector as white noise.
7. Growth plots cover the meaningful potential domain of the selected raw
   kernels instead of assuming `[0,1]`.
8. The exact candidate binaries pass automated tests and a real visual
   keyboard/mouse journey before one normal stable GitHub Release is
   published. No alpha, beta, RC, or GitHub Pre-release is created.

## Tiling construction and constraint model

### Empty start and presets

Entering a new custom Tiling draft shows an empty canvas with two equally
visible choices: choose a preset or draw the first polygon. Presets instantiate
ordinary editable polygons, lattice vectors, and seam relations. They do not
select a separate simulation implementation.

Polygon construction rejects an invalid operation when it is attempted:

- a new vertex cannot coincide with the previous vertex;
- a new segment cannot have zero length;
- a new segment cannot cross a non-adjacent segment;
- closing requires at least three distinct vertices;
- click-first-vertex, double-click, and Enter close the same construction;
- Ctrl+Z removes the last construction vertex before it touches committed
  Workbench history.

### Strict edge-to-edge seams

A seam relation pairs one complete directed polygon edge with one complete
oppositely directed edge plus an integer periodic offset. Their two endpoint
pairs are the entire relation. An endpoint landing in another edge's interior
is a T-junction and is rejected with a visible diagnostic. Partial collinear
overlap, edge crossing, gaps, and positive-area overlap are also invalid.

The validator still operates on the quotient torus, but it no longer inserts T
endpoints or splits long edges to legalize them. Every canonical boundary edge
must have exactly one full-length twin.

### Assisted solve and linked editing

Users may draw approximate polygons and place approximate neighbors. The
assistant proposes candidate full-edge pairings using endpoint distance,
opposite direction, similar length, and periodic-offset consistency. A user
confirms a proposal before it becomes a constraint.

With seam correspondence fixed, endpoint equality up to lattice translation is
a linear constraint. The solver minimizes squared displacement from the user's
drawn vertices and lattice vectors while satisfying confirmed seams exactly.
It reports under-constrained, contradictory, inverted, or degenerate systems
without committing a partial result.

After a successful solve, seam-equivalent endpoint images form one constraint
class. Dragging any member sets a positional target and re-solves the connected
system, so every linked vertex moves consistently and the tiling remains
edge-to-edge. Undo restores the complete pre-solve or pre-drag geometry in one
step.

## Kernel data and editing

### Support is first-class

Each periodic source-basis plane has values and an equally sized active mask.
The invariant is:

```text
mask[index] == false  =>  values[index] == 0
```

Deserialization, resizing, preset generation, and deactivation enforce this
invariant. The runtime skips inactive entries. Hidden non-zero values are not
preserved.

The central editor has two explicit tools:

- **Weights:** select, drag-paint, secondary-click zero, wheel adjust, and exact
  numeric input. Inactive cells cannot be changed in this tool.
- **Support:** primary-click activates a cell at zero; secondary-click
  deactivates and clears it. Drag applies the chosen support state.

Inactive cells use a distinct subdued fill and hatch/cross marker; active zero
cells use the black board background. The Inspector always displays offset,
source basis, `active: yes/no`, and weight or `—`. A compact legend remains
visible.

### Precise values and navigation

Selecting an active cell then pressing Enter or E opens a central inline
floating-point editor with a visible cursor. Double-click opens the same
editor. Enter commits, Escape cancels, and invalid/non-finite/out-of-range
input remains editable with a diagnostic.

Wheel steps remain `0.05`, Shift+wheel `0.005`, and Ctrl+wheel `0.5`.
Arrow navigation follows geometric nearest neighbors in rendered world space,
not rectangular array adjacency alone. Empty-canvas wheel zooms; inactive
cells count as empty in Weights mode and as cells in Support mode.

### Dimensions, anchor, and shape

Resize opens fields for width, height, anchor x, and anchor y within the
existing 1..129 limits. Values are remapped by lattice offset relative to the
anchor. Expansion creates inactive zero cells. Shrinking previews the active
non-zero entries that would be discarded and requires confirmation.

Support presets include full rectangle, lattice circle/ellipse, lattice ring,
world circle/ellipse, world ring, and clear. Applying a support preset changes
only the mask and clears newly inactive values. A separate value preset fills
only active cells.

## Kernel preset geometry

Every generated value/support preset declares a sampling metric:

```rust
enum KernelSamplingMetric {
    LatticeAffine,
    WorldEuclidean,
}
```

For lattice offset `n = (i,j)`, target basis `t`, and source basis `s`:

```text
lattice sample: u = (i / rx, j / ry)
world sample:   d = i*a + j*b + site(s) - site(t)
```

`a` and `b` are the confirmed periodic translation vectors. `site(basis)`
is the area centroid of the transformed basis polygon in world coordinates.
The coordinate remains meaningful even when the centroid lies outside a
concave polygon; it is a reference site, not a hit-test point. Screen pixels,
zoom, pan, and terminal cell dimensions never enter sampling.

An isotropic world Gaussian is:

```text
w = amplitude * exp(-0.5 * dot(d,d) / (sigma*sigma))
```

Consequently the six nearest cells of a regular hexagonal lattice have equal
weight. World ring and world support radius use the same distance. Anisotropic
world presets use a world-space rotation angle and two radii; they do not
inherit lattice shear.

Preset controls show metric, amplitude, sigma/radii, support radius, and target
source-basis plane. The preview uses actual polygons and updates before commit.
Preset generation never silently normalizes weights. Existing explicit
normalization remains an opt-in property and the Inspector shows raw sum,
absolute sum, minimum, and maximum.

## Simulation, Channels, and Growth presentation

An applied basis experiment renders each repeated source polygon at its actual
world position and fills it from its basis/channel state. Domain interior is
pure black. Exterior remains the existing dark navy. A regular-hexagon
experiment must visibly remain hexagonal in Simulation.

Channels renders the latest authoritative state using the same polygon scene as
Simulation. Composite, Solo, and Grid are presentation modes over that state.
Draft initial values appear only in an explicitly labeled initialization
preview. One channel defaults near-white; exactly three default to RGB; custom
colors remain unchanged.

The growth plot derives a suggested input range from the selected kernel's raw
support and source channel range. For a source constrained to `[lo,hi]`, it
shows the conservative weighted-sum interval by sign. The user may override
plot min/max without changing simulation semantics. For six unit neighbors the
default potential plot therefore includes `0..6`, not only `0..1`.

## Interaction and rendering invariants

All editor graphics and hit tests consume the same immutable scene transform.
Section changes, zoom, resize, and asynchronous frame production must never
present an old Kitty image in a new text layout. Mask, preset, resize, solver,
and numeric operations each create one undoable draft command.

Half-block mode exposes the same tools and state changes. Reduced visual
precision is acceptable; missing interaction is not.

## Verification and release

Automated tests cover mask/value invariants, resizing by offset, both sampling
metrics, hexagonal equal-distance shells, strict no-T validation, solver
propagation, raw potential plot range, actual polygon state rendering, input
mapping, Kitty placement cleanup, protocol round trips, CPU/CUDA parity, and
release workflow contracts.

The visual Agent must start from an empty custom tiling, draw and close a
triangle, Undo/Redo construction, solve its strict seams, drag one linked
vertex, edit support and exact kernel weights, generate both affine and world
Gaussians on a regular hexagon, inspect equal neighbor values, edit a growth
body against the expanded plot range, add and color channels, Apply & Run, and
visually confirm polygon simulation in both Kitty and half-block modes. It then
runs a sustained mixed-interaction cleanup audit.

Candidate binaries come from private GitHub Actions artifacts tied to one
commit. They are downloaded on the ARM64 client and tinker; the Raspberry Pi
never builds Cellarium. After the exact commit passes all gates, one stable tag
is created. CI may stage a draft Release for exact-asset smoke testing, but it
must not mark it as a GitHub Pre-release and must not create RC tags. The tested
draft is published unchanged as the stable release.
