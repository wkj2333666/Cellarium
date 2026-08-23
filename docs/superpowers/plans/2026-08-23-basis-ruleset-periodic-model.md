# Basis RuleSet and Periodic Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce the basis-aware RuleSet schema and a robust periodic arrangement validator that accepts atomic edge-to-edge T-junctions and compiles stable adjacency.

**Architecture:** Keep `TileId` wire-compatible and export it as semantic `BasisId`. Add a normalized RuleLibrary alongside legacy fields during migration, then make normalized specs authoritative. Replace fixed-neighborhood whole-edge matching with bounded candidate enumeration, adaptive predicates, atomic segment splitting, a toroidal half-edge arrangement, and exact-once coverage checks.

**Tech Stack:** Rust 2024, serde/RON, `robust = "1"` adaptive predicates, BTreeMap/BTreeSet, existing simulation and Workbench models.

**Spec:** `docs/superpowers/specs/2026-08-23-basis-aware-workbench-agentic-validation-design.md`

## Global Constraints

- All Cargo commands run on tinker; no local ARM64 build or test is permitted.
- Existing direct/raster Lenia and Conway specs must migrate without numerical behavior changes.
- Defaults contain exactly one channel and one kernel; additions are explicit.
- Kernel input count and growth ordinary-input count are equal for every RuleSet.
- T-junctions are valid only after the long shape edge is split into atomic edges.
- Fixed periodic copy ranges and unbounded geometry work are forbidden.

---

### Task 1: Stable RuleLibrary schema

**Files:**
- Create: `src/sim/ruleset.rs`
- Create: `src/sim/basis_kernel.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/sim/experiment_model.rs`
- Test: inline tests in the new modules and `experiment_model.rs`

**Interfaces:**
- Produces: `pub use TileId as BasisId` in `tiling::model` exports, retaining the existing tuple-struct constructor and wire representation.
- Produces: `RuleSetId(pub u32)`, `BindingKey { basis: BasisId, output: ChannelId }`, `RuleKernel`, `RuleSet`, `RuleBinding`, and `RuleLibrary`.
- Produces: `PeriodicKernelDefinition::weight(offset: [i16; 2], basis: BasisId) -> Option<f32>`.
- Produces: `ExperimentSpec::normalize_rules(self) -> Result<Self, Vec<ExperimentModelError>>`.

- [ ] **Step 1: Write failing schema and invariant tests**

```rust
#[test]
fn default_has_one_channel_one_basis_one_kernel() {
    let spec = ExperimentSpec::single_channel_lenia(8, 8).normalize_rules().unwrap();
    let basis = spec.basis_ids();
    assert_eq!(basis.len(), 1);
    let binding = spec.rules.binding(basis[0], spec.channels[0].id).unwrap();
    assert_eq!(spec.rules.get(binding.rule_set).unwrap().kernels.len(), 1);
}

#[test]
fn growth_arity_is_kernel_arity() {
    let mut rule = RuleSet::identity(RuleSetId(1), ChannelId(0));
    rule.kernels.push(RuleKernel::identity(KernelId(2), "outer", ChannelId(0)));
    assert!(matches!(rule.validate(), Err(RuleSetError::GrowthKernelMismatch { .. })));
}
```

- [ ] **Step 2: Run RED on tinker**

Run: `cargo test --lib sim::ruleset sim::basis_kernel sim::experiment_model::tests::default_has_one_channel_one_basis_one_kernel`

Expected: FAIL with unresolved RuleLibrary types.

- [ ] **Step 3: Implement the normalized types**

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleLibrary {
    pub defaults: BTreeMap<ChannelId, RuleSetId>,
    pub sets: Vec<RuleSet>,
    pub bindings: Vec<RuleBinding>,
}

pub struct RuleSet {
    pub id: RuleSetId,
    pub shared_name: Option<String>,
    pub kernels: Vec<RuleKernel>,
    pub growth: GrowthSource,
}

pub struct RuleBinding {
    pub basis: BasisId,
    pub output: ChannelId,
    pub rule_set: RuleSetId,
}
```

`RuleKernel` has one `source_channel` and `KernelSpatialDefinition::{Raster(KernelDefinition), Periodic(PeriodicKernelDefinition)}`. Periodic weights are a `BTreeMap<BasisId, BasisWeightPlane>`; every plane has exactly `width * height` explicit values and optional mask.

- [ ] **Step 4: Add strict validation**

Validate stable-ID uniqueness, exactly one binding for every non-frozen `(basis, channel)`, no binding for frozen output, one default per non-frozen channel, existing referenced sets, finite weights, enabled basis references, stable unique symbols, ordered `growth.kernel_inputs`, and exact kernel/growth arity.

- [ ] **Step 5: Run GREEN and full model tests**

Run: `cargo test --lib sim::ruleset sim::basis_kernel sim::experiment_model`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/sim/mod.rs src/sim/ruleset.rs src/sim/basis_kernel.rs src/sim/experiment_model.rs
git commit -m "feat: add basis-aware ruleset model"
```

### Task 2: Legacy migration and copy-on-write defaults

**Files:**
- Modify: `src/sim/experiment_model.rs`
- Modify: `src/workbench/state.rs`
- Modify: `src/workbench/command.rs`
- Test: inline tests in those files

**Interfaces:**
- Produces: `RuleLibrary::detach(binding: BindingKey) -> Result<RuleSetId, RuleSetError>`.
- Produces: `RuleLibrary::reset_to_default(binding: BindingKey) -> Result<(), RuleSetError>`.
- Produces: `RuleLibrary::edit_default(channel: ChannelId, edit: impl FnOnce(&mut RuleSet))`.
- Produces: Workbench selection `(BasisId, ChannelId, RuleSetId, KernelId)`.

- [ ] **Step 1: Write failing migration and sharing tests**

```rust
#[test]
fn legacy_global_rule_is_shared_by_all_existing_bases() {
    let legacy = octagon_square_legacy_spec();
    let normalized = legacy.normalize_rules().unwrap();
    let ids: BTreeSet<_> = normalized.rules.bindings.iter().map(|b| b.rule_set).collect();
    assert_eq!(ids.len(), 1);
}

#[test]
fn local_edit_detaches_whole_ruleset() {
    let mut state = basis_fixture_state();
    let sibling_before = state.rule_for(BasisId(1), ChannelId(0)).clone();
    state.detach_selected_ruleset().unwrap();
    state.set_selected_kernel_weight([0, 0], BasisId(0), 0.25).unwrap();
    assert_eq!(state.rule_for(BasisId(1), ChannelId(0)), &sibling_before);
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test --lib legacy_global_rule_is_shared local_edit_detaches_whole_ruleset`

- [ ] **Step 3: Implement one-way normalization**

Add serde-defaulted normalized fields and retain legacy `kernels`/`growth` only for reading. `normalize_rules` converts legacy data, clears legacy vectors, and saved normalized specs omit empty legacy fields. Specs with both nonempty legacy and normalized rule data are rejected as ambiguous.

- [ ] **Step 4: Implement semantic Workbench commands**

Add commands for `DetachRuleSet`, `ResetRuleSetToDefault`, `ReplaceRuleSet`, `AddKernel`, and `RemoveKernel`. Each command stores its exact inverse; do not route these through a generic `ReplaceDraft` once the typed command exists. Adding/removing a kernel updates `growth.kernel_inputs` atomically in the same command.

- [ ] **Step 5: Verify migration, Undo/Redo, and RON round trips**

Run: `cargo test --lib sim::experiment_model workbench::state workbench::command`

Expected: PASS, including custom sharing-name persistence and default relinking.

- [ ] **Step 6: Commit**

```bash
git add src/sim/experiment_model.rs src/workbench/state.rs src/workbench/command.rs
git commit -m "feat: migrate and share polygon rulesets"
```

### Task 3: Dynamic periodic-copy bounds and robust predicates

**Files:**
- Create: `src/sim/tiling/predicates.rs`
- Create: `src/sim/tiling/copies.rs`
- Modify: `src/sim/tiling/mod.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Produces: `LatticeCopyBounds::for_aabb(a: Vec2, b: Vec2, source: Aabb, target: Aabb, budget: GeometryBudget) -> Result<Self, TilingDiagnostic>`.
- Produces: `segment_relation(a0, a1, b0, b1) -> SegmentRelation::{Disjoint, Endpoint, ProperCrossing, TEndpoint, CollinearOverlap}`.
- Produces: `GeometryBudget::interactive()` and `GeometryBudget::authoritative()` with explicit numeric limits.

- [ ] **Step 1: Add property tests that defeat fixed neighborhoods**

Test scales `1e-6`, `1`, and `1e6`, skew bases, polygons crossing more than two periods, and a copy whose required offset magnitude exceeds two. Assert the returned bounds include every brute-force intersecting copy in a bounded test oracle.

- [ ] **Step 2: Add predicate classification tests**

Include reversed endpoints, almost-collinear large coordinates, a T endpoint in the interior, a proper crossing, and collinear partial overlap. Assert results remain unchanged under translation, rotation, and uniform scale.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib sim::tiling::copies sim::tiling::predicates`

- [ ] **Step 4: Implement inverse-lattice AABB enumeration and adaptive orientation**

Use the inverse matrix coefficients derived from `det(a,b)` to transform target/source AABB corner differences into lattice-coordinate intervals. Apply checked integer expansion and reject a candidate count above the configured budget before allocating. Use the `robust` crate for orientation signs; use scale-aware distance only to merge metrically coincident endpoints after topology classification.

- [ ] **Step 5: Run GREEN and bounded-time adversarial tests**

Run: `cargo test --lib sim::tiling::copies sim::tiling::predicates`

Expected: PASS; the million-copy fixture returns `budget_candidate_copies` without iterating a million entries.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/sim/tiling/mod.rs src/sim/tiling/predicates.rs src/sim/tiling/copies.rs
git commit -m "feat: bound robust periodic geometry queries"
```

### Task 4: Atomic edges, T-junctions, and toroidal DCEL

**Files:**
- Create: `src/sim/tiling/arrangement.rs`
- Replace internals: `src/sim/tiling/half_edge.rs`
- Modify: `src/sim/tiling/model.rs`
- Modify: `src/sim/tiling/mod.rs`

**Interfaces:**
- Produces: `PeriodicArrangement::build(&PeriodicTilingDraft, GeometryBudget) -> Result<Self, Vec<TilingDiagnostic>>`.
- Produces: `AtomicEdge { source: ShapeEdgeRef, interval: [f64; 2], start: VertexId, end: VertexId, twin: HalfEdgeId, offset: [i32; 2] }`.
- Produces: `PeriodicArrangement::neighbor_ring(BasisId) -> Vec<NeighborPlacement>`.

- [ ] **Step 1: Write failing square, hexagon, octagon-square, and T-junction fixtures**

The T fixture must contain one long boundary edge paired with two shorter opposite edges. Assert the long shape edge becomes two atomic edges, both have one twin, and the neighbor ring contains the two distinct adjacent bases.

- [ ] **Step 2: Write failing invalid fixtures**

Cover proper crossings, unmatched atomic edges, two competing twins, incompatible collinear overlap, zero-length fragments, and budget overflow. Assert stable diagnostic codes and object paths.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib sim::tiling::arrangement sim::tiling::half_edge`

- [ ] **Step 4: Implement split collection and canonical vertices**

For every shape segment, gather parameters `0`, `1`, endpoint/T parameters, and collinear-overlap endpoints; sort with `total_cmp`, merge metric duplicates, and emit nonzero atomic intervals. Canonicalize vertex position plus integer periodic offset before creating half-edges.

- [ ] **Step 5: Pair atomic half-edges and traverse torus faces**

Pair only equal geometry with opposite direction and a unique periodic offset. Sort outgoing half-edges by robust angular order, assign `next`, traverse each face once, and retain provenance back to basis polygon and shape edge for diagnostics and editor highlighting.

- [ ] **Step 6: Run GREEN**

Run: `cargo test --lib sim::tiling::arrangement sim::tiling::half_edge`

- [ ] **Step 7: Commit**

```bash
git add src/sim/tiling/model.rs src/sim/tiling/mod.rs src/sim/tiling/arrangement.rs src/sim/tiling/half_edge.rs
git commit -m "feat: validate atomic periodic tiling seams"
```

### Task 5: Exact-once coverage, Euler checks, and compiled adjacency

**Files:**
- Replace internals: `src/sim/tiling/coverage.rs`
- Modify: `src/sim/tiling/compile.rs`
- Modify: `src/sim/tiling/presets.rs`
- Create: `tests/fixtures/tiling/*.ron`

**Interfaces:**
- Produces: `validate_periodic_tiling(&PeriodicTilingDraft, GeometryBudget) -> Result<TilingValidationReport, Vec<TilingDiagnostic>>`.
- Produces: report fields `coverage_multiplicity`, `face_area`, `patch_area`, `euler_characteristic`, `atomic_edges`, and `neighbor_ring`.
- `compile_tiling` consumes the validated arrangement rather than recomputing fuzzy whole-edge matches.

- [ ] **Step 1: Add exact-once and Euler tests**

Assert valid fixtures have multiplicity one, `abs(face_area - abs(det(a,b))) <= tolerance`, and `V-E+F == 0`. Assert gap, overlap, duplicate face, and triple-overlap fixtures fail even when pairwise area subtraction would appear plausible.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib sim::tiling::coverage sim::tiling::compile`

- [ ] **Step 3: Implement arrangement-derived coverage**

Clip triangulated representative faces to the fundamental parallelogram using dynamic copies, accumulate signed coverage events, and require multiplicity one on every arrangement face. Do not compute union area by subtracting pairwise overlaps.

- [ ] **Step 4: Compile deterministic CSR**

Sort basis IDs and neighbor entries by `(target_basis, lattice_offset, source_basis)`. Emit one adjacency entry per paired atomic seam, preserving multiple atomic contacts when topological mode requires them and area/length metadata for geometric normalization.

- [ ] **Step 5: Run all tiling and structure tests**

Run: `cargo test --lib sim::tiling sim::experiment_model`

Expected: PASS with square, triangle, regular hexagon, honeycomb, octagon-square, valid T-junction, and every invalid fixture.

- [ ] **Step 6: Commit**

```bash
git add src/sim/tiling tests/fixtures/tiling
git commit -m "feat: prove periodic tiling coverage and adjacency"
```

### Task 6: Versioned persistence and protocol round trip

**Files:**
- Modify: `src/remote.rs`
- Modify: `src/sim/service.rs`
- Modify: `tests/remote_e2e.rs`
- Modify: `tests/workbench_e2e.rs`

**Interfaces:**
- Bumps: `PROTOCOL_VERSION` from `8` to `9`.
- Apply and ExperimentState continue carrying normalized `ExperimentSpec`; decoded legacy fields are normalized before service validation.
- Diagnostics use stable paths including `basis/<id>/channel/<id>/ruleset/<id>/kernel/<id>`.

- [ ] **Step 1: Add failing round-trip tests for two bases, three channels, shared/default/local RuleSets, two kernels, and a T-junction**

- [ ] **Step 2: Run RED**

Run: `cargo test --lib remote::tests::basis_ruleset_apply_round_trip sim::service::tests::basis_diagnostics_have_stable_paths`

- [ ] **Step 3: Extend protocol serialization and service normalization**

Reject version 8 peers clearly, bound RuleSet/kernel/plane/vector counts before large allocations, normalize once at the Apply boundary, and return the complete normalized authoritative model in ApplyAccepted and ExperimentState.

- [ ] **Step 4: Run protocol and non-network integration tests**

Run: `cargo test --lib remote sim::service && cargo test --test remote_e2e -- --skip tinker && cargo test --test workbench_e2e`

- [ ] **Step 5: Commit**

```bash
git add src/remote.rs src/sim/service.rs tests/remote_e2e.rs tests/workbench_e2e.rs
git commit -m "feat: synchronize basis rulesets over protocol v9"
```

Expected final boundary: normalized basis-aware experiments and robust periodic topology persist and round-trip authoritatively while the legacy raster runtime still behaves exactly as before.

