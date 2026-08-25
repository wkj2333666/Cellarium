# Stable Workbench Geometry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship one stable Cellarium release whose tiling, periodic-kernel, channel, growth-plot, and simulation interfaces operate on the real polygon geometry and pass a real keyboard/mouse visual journey.

**Architecture:** Keep `ExperimentSpec` and the basis-sparse runtime authoritative. Add first-class periodic support masks and persistent preset metadata, generate weights in either lattice or world geometry, constrain tilings with strict full-edge seam relations, and share one polygon-state scene between Simulation and Channels. Candidate binaries are private workflow artifacts; one exact tested commit becomes one non-prerelease stable tag.

**Tech Stack:** Rust 2024, serde/RON, existing CPU/CUDA basis runtime, Ratatui, Kitty graphics, Xvfb/Openbox/Kitty/xdotool, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-08-25-stable-workbench-geometry-design.md`

## Global Constraints

- Do not create alpha, beta, RC, or GitHub Pre-release versions.
- Do not build Cellarium on the Raspberry Pi.
- Build, unit-test, GPU-test, and performance-test on tinker or GitHub Actions.
- Preserve direct mode, C/S mode, Kitty graphics, and interactive half-block fallback.
- Periodic masked entries always store zero and never enter convolution.
- Kernel preset generation never silently normalizes raw weights.
- T-junctions are invalid; only complete edge-to-edge seam pairs are legal.
- Every production change starts with a failing focused test on tinker.
- Do not use subagents for this implementation.

---

### Task 1: Periodic support invariant and editable dimensions

**Files:**
- Modify: `src/sim/basis_kernel.rs`
- Modify: `src/sim/ruleset.rs`
- Modify: `src/workbench/command.rs`
- Modify: `src/workbench/state.rs`
- Test: inline tests in the files above

**Interfaces:**
- Produces: `PeriodicKernelDefinition::is_active(offset, basis) -> Option<bool>`.
- Produces: `PeriodicKernelDefinition::set_active(offset, basis, active) -> Result<(), BasisKernelError>`.
- Produces: `PeriodicKernelDefinition::resize(width, height, anchor_x, anchor_y) -> Result<ResizeReport, BasisKernelError>`.
- Produces: `ResizeReport { discarded_active_nonzero: Vec<KernelEntry> }`.

- [ ] **Step 1: Write failing support-invariant tests**

```rust
#[test]
fn inactive_periodic_weight_is_zero_and_cannot_be_written() {
    let mut kernel = fixture_with_mask(false, 0.75);
    kernel.canonicalize().unwrap();
    assert_eq!(kernel.raw_weight([0, 0], BasisId(0)), Some(0.0));
    assert_eq!(kernel.weight([0, 0], BasisId(0)), None);
    assert_eq!(
        kernel.set_weight([0, 0], BasisId(0), 0.5),
        Err(BasisKernelError::InactiveEntry { basis: BasisId(0), offset: [0, 0] })
    );
}
```

- [ ] **Step 2: Run RED on tinker**

Run: `cargo test --lib sim::basis_kernel::tests::inactive_periodic_weight_is_zero_and_cannot_be_written`

Expected: compile failure because the canonicalization and inactive-entry APIs do not exist.

- [ ] **Step 3: Implement canonical support APIs**

Validation rejects malformed lengths/non-finite values, canonicalization clears
masked values, `set_weight` rejects inactive entries, activation initializes
zero, and deactivation clears before returning.

- [ ] **Step 4: Write resize-by-offset tests**

```rust
#[test]
fn resize_preserves_weights_by_lattice_offset() {
    let mut kernel = three_by_three_anchor_one();
    kernel.set_weight([1, -1], BasisId(0), 0.4).unwrap();
    let report = kernel.resize(5, 3, 2, 1).unwrap();
    assert!(report.discarded_active_nonzero.is_empty());
    assert_eq!(kernel.weight([1, -1], BasisId(0)), Some(0.4));
    assert_eq!(kernel.is_active([-2, 0], BasisId(0)), Some(false));
}
```

- [ ] **Step 5: Implement resize and undoable Workbench commands**

Remap every plane by `offset = [x-anchor_x,y-anchor_y]`. Expansion creates
masked zero entries. A shrink report is returned before committing and the
Workbench records one `ReplaceDraft` command only after confirmation.

- [ ] **Step 6: Run Task 1 GREEN and commit**

Run: `cargo test --lib sim::basis_kernel workbench::state workbench::command`

```bash
git add src/sim/basis_kernel.rs src/sim/ruleset.rs src/workbench/command.rs src/workbench/state.rs
git commit -m "feat: make periodic kernel support explicit"
```

### Task 2: Kernel Support/Weights tools and precise editing

**Files:**
- Modify: `src/workbench/kernel_editor.rs`
- Modify: `src/workbench/numeric_editor.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `src/app.rs`
- Modify: `tests/workbench_e2e.rs`

**Interfaces:**
- Consumes: Task 1 support and resize APIs.
- Produces: `KernelTool::{Weights, Support}`.
- Produces: `PeriodicKernelScene::hit_test(point, tool) -> Option<KernelSelection>`.
- Produces: central `KernelNumericOverlay` commit/cancel behavior.

- [ ] **Step 1: Write failing interaction-controller tests**

Assert that wheel over an inactive cell zooms in Weights mode, primary click
activates it in Support mode, deactivation clears its value, double-click and E
open the same editor, and one support drag creates one undo entry.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib app::tests::periodic_support_tool app::tests::inactive_weight_wheel_zooms && cargo test --test workbench_e2e kernel_support_and_exact_value`

- [ ] **Step 3: Render explicit states and legend**

Render inactive cells with a subdued cross marker, active zero as pure black,
positive cyan, negative warm red, target gold, and selection white. Add a
Weights/Support toolbar control and Inspector fields:

```text
offset: [dx,dy] · source basis N
active: yes|no
weight: value|—
```

- [ ] **Step 4: Implement tools, resize dialog, presets entry point, and exact overlay**

Use the same scene transform for hit testing. Arrow selection uses nearest
polygon centroid in the requested direction. E, Enter, and double-click open a
central numeric overlay for active entries. R opens width/height/anchor fields;
destructive shrink presents the Task 1 report and requires confirmation.

- [ ] **Step 5: Verify mouse and keyboard parity**

Run: `cargo test --lib workbench::kernel_editor app::tests::periodic && cargo test --test workbench_e2e kernel`

- [ ] **Step 6: Commit**

```bash
git add src/workbench/kernel_editor.rs src/workbench/numeric_editor.rs src/workbench/state.rs src/tui/workbench.rs src/app.rs tests/workbench_e2e.rs
git commit -m "feat: complete periodic kernel editing tools"
```

### Task 3: Affine and world-space kernel generators

**Files:**
- Modify: `src/sim/basis_kernel.rs`
- Create: `src/sim/kernel_sampling.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/workbench/kernel_editor.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `src/remote.rs`
- Test: inline unit tests and `tests/remote_e2e.rs`

**Interfaces:**
- Produces: `KernelSamplingMetric::{LatticeAffine, WorldEuclidean}`.
- Produces: `KernelProfile::{Gaussian, Ring, Constant}`.
- Produces: `KernelGenerationSpec { metric, profile, amplitude, sigma, radii, angle, support_radius }`.
- Produces: `generate_periodic_plane(tiling, target_basis, source_basis, definition, spec) -> BasisWeightPlane`.

- [ ] **Step 1: Write failing geometry tests**

```rust
#[test]
fn world_gaussian_gives_six_hex_neighbors_equal_weight() {
    let generated = generate_periodic_plane(
        &regular_hexagon_tiling(),
        BasisId(0),
        BasisId(0),
        &seven_cell_stencil(),
        &world_gaussian(1.0),
    ).unwrap();
    let neighbors = six_nearest_neighbor_weights(&generated);
    assert!(neighbors.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 1e-6));
}

#[test]
fn affine_and_world_metrics_differ_on_oblique_lattice() {
    assert_ne!(
        sample_fixture(KernelSamplingMetric::LatticeAffine),
        sample_fixture(KernelSamplingMetric::WorldEuclidean)
    );
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test --lib sim::kernel_sampling`

- [ ] **Step 3: Implement stable world geometry**

Compute transformed polygon area centroids once. For each lattice offset use
`d = i*a + j*b + site(source)-site(target)`. Isotropic Gaussian uses
`amplitude*exp(-0.5*dot(d,d)/sigma²)`; world support uses the same norm.
Affine mode preserves the existing normalized lattice-coordinate behavior.
Reject non-finite/degenerate parameters and clear values outside support.

- [ ] **Step 4: Implement graphics preview and persistent editor state**

The preset panel chooses metric/profile/source basis and edits numeric
parameters through the existing numeric editor. Preview actual polygons before
one atomic Apply. Show raw sum, absolute sum, min, and max. Manual edits turn
the displayed provenance into `Custom` without altering values.

- [ ] **Step 5: Verify serialization, protocol, and runtime raw values**

Run: `cargo test --lib sim::kernel_sampling remote workbench::kernel_editor && cargo test --test remote_e2e -- --skip tinker`

Assert a six-unit-neighbor kernel compiles to six weights of exactly 1.0 and is
not normalized.

- [ ] **Step 6: Commit**

```bash
git add src/sim/basis_kernel.rs src/sim/kernel_sampling.rs src/sim/mod.rs src/workbench/kernel_editor.rs src/workbench/state.rs src/tui/workbench.rs src/remote.rs tests/remote_e2e.rs
git commit -m "feat: sample kernel presets in lattice or world geometry"
```

### Task 4: Strict edge-to-edge tiling assistance

**Files:**
- Modify: `src/sim/tiling/model.rs`
- Modify: `src/sim/tiling/arrangement.rs`
- Modify: `src/sim/tiling/half_edge.rs`
- Create: `src/sim/tiling/constraints.rs`
- Create: `src/sim/tiling/solver.rs`
- Modify: `src/sim/tiling/mod.rs`
- Modify: `src/workbench/tiling_editor.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/app.rs`
- Modify: `tests/fixtures/tiling/t_junction.ron`
- Modify: `tests/workbench_e2e.rs`

**Interfaces:**
- Produces: `SeamConstraint { lhs: EdgeRef, rhs: EdgeRef, periodic_offset: [i32;2] }`.
- Produces: `propose_full_edge_seams(draft, tolerance) -> Vec<SeamProposal>`.
- Produces: `solve_edge_constraints(draft, constraints, drag_target) -> Result<SolvedTiling, SolveDiagnostic>`.

- [ ] **Step 1: Write failing strict-validator tests**

Change the T-junction fixture expectation from valid to
`TilingDiagnostic::TJunction`. Add full-edge hexagon and octagon-square
fixtures that remain valid. Assert partial collinear overlap is not paired.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib sim::tiling::arrangement sim::tiling::half_edge`

- [ ] **Step 3: Remove T splitting and require full twins**

Keep robust orientation/intersection predicates, but reject an endpoint in a
non-endpoint edge interior. Build half-edges only from original full boundary
edges and require exactly one reversed twin with a consistent periodic offset.

- [ ] **Step 4: Write failing solver and construction tests**

Assert an approximate triangle closes through click-first/Enter/double-click,
Ctrl+Z removes its latest uncommitted vertex, confirmed seam solve removes gaps,
and dragging one constrained vertex moves every equivalent endpoint while
validation stays green.

- [ ] **Step 5: Implement bounded proposal and solve**

Generate candidate full-edge pairs under hard count budgets. Build endpoint
equality equations including periodic offsets, retain user coordinates as
least-squares targets, solve with scaled pivoting, reject rank deficiency and
inversion, and commit atomically. Drag adds one high-weight target and resolves
the same connected constraint system.

- [ ] **Step 6: Implement empty start, visible preset choice, proposals, and linked drag**

No polygon is silently created for a custom draft. The canvas shows Draw and
Preset actions. Invalid construction clicks are rejected immediately with a
local visual diagnostic. Confirmed seams and solver displacement are previewed
before Apply.

- [ ] **Step 7: Verify and commit**

Run: `cargo test --lib sim::tiling workbench::tiling_editor app::tests::tiling && cargo test --test workbench_e2e tiling`

```bash
git add src/sim/tiling src/workbench/tiling_editor.rs src/workbench/state.rs src/app.rs tests/fixtures/tiling tests/workbench_e2e.rs
git commit -m "feat: solve strict edge-to-edge tilings"
```

### Task 5: Polygon simulation, Channels, and meaningful growth domains

**Files:**
- Create: `src/render/basis_scene.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/workbench/channel_editor.rs`
- Modify: `src/workbench/growth_graph.rs`
- Modify: `src/sim/growth/plot.rs`
- Modify: `src/tui/workbench.rs`
- Modify: `tests/workbench_e2e.rs`
- Modify: `tests/remote_e2e.rs`

**Interfaces:**
- Produces: `BasisStateScene::from_snapshot(tiling, layout, state, palette, view)`.
- Produces: `potential_interval(weights, source_interval) -> [f32;2]`.
- Simulation and Channels consume the same `BasisStateScene`.

- [ ] **Step 1: Write failing scene and range tests**

Assert a regular-hexagon snapshot rasterizes six-sided filled polygons at
periodic positions, Channels Composite uses snapshot values rather than
`channel.initial`, and six unit weights over `[0,1]` produce `[0,6]`.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib render::basis_scene workbench::channel_editor sim::growth::plot`

- [ ] **Step 3: Implement shared polygon-state scene**

Map every lattice site/basis/channel index through `StateLayout`, repeat the
actual transformed basis polygon, clip before rasterization, and blend channel
colors on pure-black domain interior. Keep the existing dark navy outside.

- [ ] **Step 4: Replace Channels noise and wire growth range**

Channels Composite/Solo/Grid all use the authoritative scene; initialization is
an explicitly labeled alternate preview. Growth computes a conservative raw
potential interval by weight sign and allows an editor-only min/max override.

- [ ] **Step 5: Verify C/S and direct rendering**

Run: `cargo test --lib render workbench::channel_editor sim::growth::plot && cargo test --test workbench_e2e channels growth && cargo test --test remote_e2e -- --skip tinker`

- [ ] **Step 6: Commit**

```bash
git add src/render src/app.rs src/workbench/channel_editor.rs src/workbench/growth_graph.rs src/sim/growth/plot.rs src/tui/workbench.rs tests
git commit -m "feat: render basis experiments in their true geometry"
```

### Task 6: Full remote gates and visual Agent journey

**Files:**
- Modify: `tests/agentic/full-journey.md`
- Modify: `tests/agentic/scripts/*.sh` only when a harness defect is proven
- Modify: `docs/agentic-testing.md`
- Create at runtime: `target/agentic/<run-id>/` evidence (not committed)

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: screenshots, action log, release identities, ack/generation
  correlations, sustained-run metrics, and cleanup audit.

- [ ] **Step 1: Run complete automated gates on tinker**

Run:

```bash
cargo fmt --check
cargo test --locked --all-targets
cargo test --locked --no-default-features --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --release --test remote_e2e basis_cpu_cuda_parity -- --ignored --exact --nocapture
git diff --check
```

- [ ] **Step 2: Build candidate workflow artifacts**

Push the tested commit without an RC tag. Trigger the normal CI packaging
workflow and download its Linux x86_64 and Linux aarch64 artifacts with
`gh run download`. Verify commit identity and SHA-256. Install x86_64 on
tinker and use the prebuilt aarch64 artifact on the Pi; do not build on Pi.

- [ ] **Step 3: Execute adaptive Kitty journey**

Visually locate and operate every step in the spec: empty triangle creation,
closure variants, construction Undo/Redo, strict solve, linked drag, regular
hex preset, Support/Weights tools, inactive-cell zoom, resize, exact value,
affine/world Gaussian comparison, six equal world neighbors, Channels
authoritative preview, growth `0..6` plot, Apply & Run, and hexagonal
Simulation. Each action requires a correlated new visual result, not only a
trace.

- [ ] **Step 4: Execute half-block and sustained journeys**

Repeat all state-changing operations in half-block. Then mix navigation,
painting, support edits, resize, presets, pan/zoom, text edits, Apply, section
changes, and reconnect for at least ten minutes. Require bounded interaction,
no stale placement, no crop, no coordinate drift, and zero leaked Cellarium,
Kitty, Xvfb, shared-memory, or server processes.

- [ ] **Step 5: Close every visual defect with TDD**

For each defect: retain failing before/after screenshots, add a focused failing
test on tinker, verify RED, implement one root-cause fix, verify GREEN and
adjacent suites, then restart the user journey from its first step.

- [ ] **Step 6: Commit evidence index**

```bash
git add tests/agentic/full-journey.md docs/agentic-testing.md
git commit -m "test: certify stable workbench user journeys"
```

### Task 7: One stable release, no prerelease series

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `.github/workflows/release.yml`
- Modify: `tests/workflow_contract.rs`
- Modify: `docs/releases.md`
- Modify: `README.md`

**Interfaces:**
- Produces: one stable semantic version and six architecture archives plus
  `SHA256SUMS`.
- Prohibits: prerelease package versions, RC tags, and GitHub
  `prerelease=true`.

- [ ] **Step 1: Write failing release-contract tests**

Assert package version has no prerelease component, release workflow rejects
tags not exactly `v$package_version`, GitHub Release creation omits
`--prerelease`, all six archives are checksummed, and workflow artifacts are
available before tagging.

- [ ] **Step 2: Run RED**

Run: `cargo test --test workflow_contract`

Expected: current `0.2.0-rc.23` and RC-oriented workflow fail.

- [ ] **Step 3: Implement stable staging**

Set the next stable version, update documentation, and make CI retain candidate
archives by commit. A stable tag creates a non-prerelease draft Release so the
exact assets can be smoked; publishing only flips `draft=false` and never
rebuilds or replaces assets.

- [ ] **Step 4: Repeat final gates and exact-asset smoke**

Run Task 6 Step 1 again. Tag the exact tested commit once, wait for packaging,
verify every checksum, install the exact draft assets on Pi and tinker, and
repeat the complete Kitty/half-block/direct smoke. On failure keep the draft
unpublished, fix on a new patch version commit, and do not publish a misleading
release.

- [ ] **Step 5: Publish unchanged as latest stable**

Run: `gh release edit <stable-tag> --draft=false --latest`

Verify the public asset hashes match the tested draft and run one clean
post-publication C/S smoke.

- [ ] **Step 6: Record and commit release identity**

```bash
git add Cargo.toml Cargo.lock .github/workflows tests/workflow_contract.rs docs/releases.md README.md docs/agentic-testing.md
git commit -m "release: publish stable polygon workbench"
```

Expected final boundary: users download one normal stable release whose exact
assets passed the strict geometry, kernel authoring, direct/C/S, Kitty,
half-block, CUDA, agentic interaction, and cleanup gates.
