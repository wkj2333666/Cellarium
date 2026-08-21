# Periodic Polygon Tiling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reliable headless geometry layer that represents, validates, and compiles user-defined periodic straight-sided polygon tilings—including octagon-square tiling—into render meshes, adjacency CSR, and area-aware kernel weights.

**Architecture:** Persist Cellarium-owned `f64` polygon/prototype/instance types and use `geo` only behind a geometry adapter for validity, Boolean coverage, area, centroid, affine operations, and triangulation. User-confirmed translations define the period; canonical half-edges derive adjacency and carry lattice offsets. Apply compiles immutable arrays once, so simulation steps perform no polygon operations.

**Tech Stack:** Rust 2024, `geo = 0.33.1`, serde/RON, existing topology CSR, CPU/CUDA sparse backends, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-21-interactive-experiment-workbench-design.md`

## Global Constraints

- First release supports regular and custom simple straight-sided polygons only; no curves, holes, or aperiodic tilings.
- The user confirms two non-collinear translation vectors; automatic period discovery is advisory only.
- Geometry editing and validation use `f64`; compiled simulation arrays may use `f32` after finite/range checks.
- A valid fundamental patch has no interior overlap, no uncovered area, and exactly paired internal/periodic half-edges within a scale-aware tolerance.
- Topological mode weights tiles equally; Geometric mode includes source tile area in spatial convolution.
- Polygon work is performed during edit validation or Apply, never per simulation step.
- `geo` is used for its documented validation, Boolean operations, and triangulation APIs; do not add a system GEOS dependency.

---

### Task 1: Tiling schema and polygon validity adapter

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `src/sim/tiling/mod.rs`
- Create: `src/sim/tiling/model.rs`
- Create: `src/sim/tiling/polygon.rs`
- Modify: `src/sim/mod.rs`
- Test: `src/sim/tiling/polygon.rs`

**Interfaces:**
- Produces: `TileId`, `PrototypeId`, `Vec2`, `RigidTransform`, `PrototypeShape`, `TilePrototype`, `TileInstance`, `TilingMode`, `PeriodicTilingDraft`, `PolygonIssue`.
- Produces: `prototype_vertices`, `instance_polygon`, `validate_polygon`.

- [ ] **Step 1: Add the pure-Rust geometry dependency**

Add:

```toml
geo = "0.33.1"
```

Run: `cargo check --locked`

Expected before lock update: Cargo reports that the lock file needs updating. Run `cargo update -p geo --precise 0.33.1`, then `cargo check --locked` must pass.

- [ ] **Step 2: Add failing regular/custom polygon tests**

```rust
#[test]
fn regular_octagon_has_unit_edges_and_positive_area() {
    let shape = PrototypeShape::RegularPolygon { sides: 8, side_length: 1.0 };
    let vertices = prototype_vertices(&shape).unwrap();
    assert_eq!(vertices.len(), 8);
    for edge in cyclic_edges(&vertices) {
        assert!((edge.length() - 1.0).abs() < 1e-12);
    }
    assert!(signed_area(&vertices) > 0.0);
}

#[test]
fn custom_bow_tie_is_rejected_as_self_intersecting() {
    let vertices = vec![v(0.0, 0.0), v(1.0, 1.0), v(0.0, 1.0), v(1.0, 0.0)];
    assert!(validate_polygon(&vertices).iter().any(|i| i.code == "self_intersection"));
}
```

- [ ] **Step 3: Run tests before creating tiling modules**

Run: `cargo test --locked --lib sim::tiling::polygon`

Expected: compile failure because the tiling module is absent.

- [ ] **Step 4: Add owned schema and `geo` conversion boundary**

Use these owned shapes:

```rust
pub enum PrototypeShape {
    RegularPolygon { sides: u16, side_length: f64 },
    SimplePolygon { vertices: Vec<Vec2> },
}

pub struct PeriodicTilingDraft {
    pub translation_a: Vec2,
    pub translation_b: Vec2,
    pub prototypes: Vec<TilePrototype>,
    pub instances: Vec<TileInstance>,
    pub mode: TilingMode,
}
```

Require 3 through 64 sides, positive finite side length, at least three distinct finite vertices, CCW nonzero area, and `geo::Validation` success. Convert to/from `geo::Polygon<f64>` only inside `polygon.rs`; serialized formats must not expose third-party types.

- [ ] **Step 5: Run polygon tests and cross-platform check**

Run: `cargo test --locked --lib sim::tiling::polygon && cargo check --locked --no-default-features`

Expected: all commands pass without a system geometry library.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/sim/tiling src/sim/mod.rs
git commit -m "feat: model periodic polygon tiles"
```

### Task 2: Edge snapping and canonical half-edges

**Files:**
- Create: `src/sim/tiling/half_edge.rs`
- Create: `src/sim/tiling/snap.rs`
- Modify: `src/sim/tiling/mod.rs`
- Test: `src/sim/tiling/snap.rs`
- Test: `src/sim/tiling/half_edge.rs`

**Interfaces:**
- Produces: `EdgeRef { tile: TileId, edge: u16 }`, `SnapCandidate`, `SnapResult`, `snap_edge`, `HalfEdge`, `EdgePair`, `canonical_half_edges`.

- [ ] **Step 1: Add exact snap and periodic-pair tests**

```rust
#[test]
fn snapping_moves_the_instance_so_whole_edges_coincide_oppositely() {
    let fixed = square_instance(TileId(1), v(0.0, 0.0), 0.0);
    let moving = square_instance(TileId(2), v(1.02, 0.01), 0.01);
    let snapped = snap_edge(&fixed, 1, &moving, 3, 0.05).unwrap();
    let (a0, a1) = world_edge(&fixed, 1);
    let (b0, b1) = world_edge(&snapped.instance, 3);
    assert_vec_close(a0, b1, 1e-12);
    assert_vec_close(a1, b0, 1e-12);
}

#[test]
fn opposite_fundamental_edges_pair_with_integer_offset() {
    let tiling = one_square_periodic_fixture();
    let pairs = canonical_half_edges(&tiling, tolerance(&tiling)).unwrap();
    assert!(pairs.iter().any(|p| p.lattice_offset == [1, 0]));
    assert!(pairs.iter().any(|p| p.lattice_offset == [0, 1]));
}
```

- [ ] **Step 2: Run tests before implementation**

Run: `cargo test --locked --lib sim::tiling::snap && cargo test --locked --lib sim::tiling::half_edge`

Expected: compile failure for missing modules.

- [ ] **Step 3: Implement rigid whole-edge snapping**

Reject edge-length mismatch above the caller tolerance. Compute the rotation that maps the moving edge vector to the negative fixed vector, rotate around the moving edge midpoint, then translate midpoints together. Return the changed rigid transform and paired `EdgeRef`s; never mutate prototype vertices.

- [ ] **Step 4: Canonicalize internal and periodic half-edges**

Map endpoints into lattice coordinates by inverting `[a b]`. Search translations in the bounded range occupied by instance polygons. Pair edges when endpoints agree in reverse order after an integer translation. Store the offset from source representative to target representative. An edge with zero or multiple candidates is a diagnostic; do not choose by iteration order.

- [ ] **Step 5: Run focused tests**

Run: `cargo test --locked --lib sim::tiling::snap && cargo test --locked --lib sim::tiling::half_edge`

Expected: all snapping and pairing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/sim/tiling/half_edge.rs src/sim/tiling/snap.rs src/sim/tiling/mod.rs
git commit -m "feat: snap and pair periodic tile edges"
```

### Task 3: Periodic coverage, gaps, and overlaps

**Files:**
- Create: `src/sim/tiling/coverage.rs`
- Modify: `src/sim/tiling/mod.rs`
- Test: `src/sim/tiling/coverage.rs`

**Interfaces:**
- Produces: `CoverageReport { patch_area, covered_area, overlap_area, gap_area, tolerance }`, `validate_coverage(&PeriodicTilingDraft) -> Result<CoverageReport, Vec<TilingDiagnostic>>`.

- [ ] **Step 1: Add square, gap, overlap, and scale-invariance tests**

```rust
#[test]
fn one_square_exactly_covers_its_period() {
    let report = validate_coverage(&one_square_periodic_fixture()).unwrap();
    assert!(report.gap_area <= report.tolerance);
    assert!(report.overlap_area <= report.tolerance);
}

#[test]
fn shifted_tile_reports_both_gap_and_overlap() {
    let mut tiling = one_square_periodic_fixture();
    tiling.instances[0].transform.translation.x += 0.1;
    let errors = validate_coverage(&tiling).unwrap_err();
    assert!(errors.iter().any(|e| e.code == "coverage_gap"));
    assert!(errors.iter().any(|e| e.code == "coverage_overlap"));
}

#[test]
fn validity_is_unchanged_under_uniform_scaling() {
    for scale in [1e-3, 1.0, 1e3] {
        assert!(validate_coverage(&scaled_square_fixture(scale)).is_ok());
    }
}
```

- [ ] **Step 2: Run coverage tests before implementation**

Run: `cargo test --locked --lib sim::tiling::coverage`

Expected: compile failure for the missing module.

- [ ] **Step 3: Implement bounded periodic clipping**

Build the fundamental parallelogram `[0, a, a+b, b]`. Transform each instance bounding box into lattice coordinates to derive the finite translation range whose copies can intersect the patch; reject a range exceeding 1,000,000 candidate copies. Intersect candidates with the patch using `geo::BooleanOps`, compute `sum(fragment_area)`, and compute `unary_union(fragment).unsigned_area()`.

Use:

```rust
let tolerance = (patch_area.abs() * 1e-10).max(max_basis_len.powi(2) * 1e-12);
let overlap_area = (fragment_sum - union_area).max(0.0);
let gap_area = (patch_area - union_area).max(0.0);
```

Reject when either exceeds tolerance. Coverage does not replace half-edge pairing; both validators must pass.

- [ ] **Step 4: Run coverage tests**

Run: `cargo test --locked --lib sim::tiling::coverage`

Expected: all coverage tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim/tiling/coverage.rs src/sim/tiling/mod.rs
git commit -m "feat: validate periodic tile coverage"
```

### Task 4: Compile tiling to immutable meshes and adjacency CSR

**Files:**
- Create: `src/sim/tiling/compile.rs`
- Modify: `src/sim/topology.rs`
- Modify: `src/sim/strategy.rs`
- Modify: `src/sim/tiling/mod.rs`
- Test: `src/sim/tiling/compile.rs`

**Interfaces:**
- Produces: `CompiledTiling { tile_ids, centers, areas, face_side_counts, vertex_face_cycles, triangles, offsets, neighbors, neighbor_offsets }`, `compile_tiling`.
- Produces: adapter from `CompiledTiling` adjacency to existing `CompiledTopology`/sparse execution strategy.

- [ ] **Step 1: Add deterministic CSR and mesh tests**

```rust
#[test]
fn square_period_compiles_four_periodic_neighbor_templates() {
    let compiled = compile_tiling(&one_square_periodic_fixture()).unwrap();
    assert_eq!(compiled.tile_ids.len(), 1);
    assert_eq!(compiled.offsets, vec![0, 4]);
    assert_eq!(compiled.neighbor_offsets, vec![[-1, 0], [0, -1], [0, 1], [1, 0]]);
    assert_eq!(compiled.triangles.len(), 2);
}

#[test]
fn compile_order_is_stable_under_instance_vector_reordering() {
    let a = compile_tiling(&octagon_square_fixture()).unwrap();
    let b = compile_tiling(&reversed_instances(octagon_square_fixture())).unwrap();
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run compile tests before implementation**

Run: `cargo test --locked --lib sim::tiling::compile`

Expected: compile failure for missing compiler.

- [ ] **Step 3: Build deterministic representatives and CSR**

Sort representative tiles by `TileId`. Use half-edge pairs to append `(neighbor_dense_id, lattice_offset)` entries sorted by source ID, target ID, and offset. Preserve multiplicity. Compute centroid and unsigned area from validated polygons. Use `geo::TriangulateEarcut` once and store CCW `[[f32; 2]; 3]` triangles after checked conversion.

- [ ] **Step 4: Adapt sparse strategy selection**

Keep legacy `compile_topology` intact. Add a constructor that accepts precompiled offsets/neighbors/weights from tiling and validates the same CSR invariants before CPU/CUDA allocation.

- [ ] **Step 5: Run tiling/topology/strategy tests**

Run: `cargo test --locked --lib sim::tiling::compile && cargo test --locked --lib sim::topology && cargo test --locked --lib sim::strategy`

Expected: all focused tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/sim/tiling/compile.rs src/sim/tiling/mod.rs src/sim/topology.rs src/sim/strategy.rs
git commit -m "feat: compile polygon tilings to sparse topology"
```

### Task 5: Topological and geometric kernel weight banks

**Files:**
- Create: `src/sim/tiling/kernel_weights.rs`
- Modify: `src/sim/experiment_model.rs`
- Modify: `src/sim/runtime.rs`
- Modify: `src/sim/cuda.rs`
- Test: `src/sim/tiling/kernel_weights.rs`

**Interfaces:**
- Replaces `KernelSlot.definition: KernelDefinition` with `KernelSpec::{Raster, Topological, Spatial}` while migrating Raster unchanged.
- Produces: `SparseWeightBank { offsets, source_tiles, weights }`, `compile_weight_bank`.

- [ ] **Step 1: Add equal-weight and area-weighted tests**

```rust
#[test]
fn topological_mode_does_not_scale_by_tile_area() {
    let bank = compile_weight_bank(&mixed_area_fixture(TilingMode::Topological), &unit_neighbor_kernel()).unwrap();
    assert_eq!(bank.weights_for_target(0), &[1.0, 1.0]);
}

#[test]
fn geometric_mode_multiplies_spatial_samples_by_source_area() {
    let fixture = mixed_area_fixture(TilingMode::Geometric);
    let bank = compile_weight_bank(&fixture, &constant_spatial_kernel(2.0)).unwrap();
    let weights = bank.weights_for_target(0);
    assert!((weights[0] / weights[1] - fixture.areas[bank.source_tiles[0]] / fixture.areas[bank.source_tiles[1]]).abs() < 1e-6);
}
```

- [ ] **Step 2: Run weight tests before adding kernel variants**

Run: `cargo test --locked --lib sim::tiling::kernel_weights`

Expected: compile failure for missing variants/module.

- [ ] **Step 3: Compile graph and spatial kernels**

Topological kernels perform bounded BFS by graph hops and apply the configured hop-weight profile. Spatial kernels enumerate periodic source copies whose centers fall within cutoff, evaluate the formula using distance/direction variables, multiply by source area in Geometric mode, then apply the selected normalization. Sort sources deterministically and reject non-finite weights or unsafe edge counts.

- [ ] **Step 4: Feed sparse banks to CPU/CUDA without per-step geometry**

Each `CompiledKernelInput` owns or indexes one immutable sparse weight bank. CPU gathers old channel values by target range. CUDA uploads offsets, sources, and weights once on backend construction. Multiple kernels retain separate banks even when they share routing.

- [ ] **Step 5: Run CPU/CUDA sparse parity tests**

Run: `cargo test --locked --no-default-features --lib sim::tiling::kernel_weights && cargo test --locked --no-default-features --lib sim::runtime && cargo test --locked --lib sim::cuda::tests::generic_csr_topology_step_matches_cpu_reference`

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/sim/tiling/kernel_weights.rs src/sim/experiment_model.rs src/sim/runtime.rs src/sim/cuda.rs
git commit -m "feat: compile tiling-aware kernel weights"
```

### Task 6: Built-in tiling presets, including octagon-square

**Files:**
- Create: `src/sim/tiling/presets.rs`
- Modify: `src/sim/tiling/mod.rs`
- Test: `src/sim/tiling/presets.rs`

**Interfaces:**
- Produces: `TilingPreset::{Square,HexCells,Honeycomb,OctagonSquare}`, `build_preset`.

- [ ] **Step 1: Add preset validity and topology tests**

```rust
#[test]
fn every_builtin_preset_validates_and_compiles() {
    for preset in TilingPreset::ALL {
        let draft = build_preset(preset, 1.0);
        validate_coverage(&draft).unwrap();
        canonical_half_edges(&draft, tolerance(&draft)).unwrap();
        compile_tiling(&draft).unwrap();
    }
}

#[test]
fn octagon_square_has_one_of_each_representative_and_vertex_type_488() {
    let compiled = compile_tiling(&build_preset(TilingPreset::OctagonSquare, 1.0)).unwrap();
    assert_eq!(compiled.tile_ids.len(), 2);
    assert_eq!(compiled.prototype_side_counts(), vec![4, 8]);
    assert_eq!(compiled.vertex_face_cycles(), vec![vec![4, 8, 8]]);
}
```

- [ ] **Step 2: Run preset tests before implementation**

Run: `cargo test --locked --lib sim::tiling::presets`

Expected: compile failure for missing presets.

- [ ] **Step 3: Generate exact regular presets**

For octagon-square side length `s`, set `period = s * (1.0 + sqrt(2.0))`, translations `[period, 0]` and `[0, period]`. Center the octagon at the origin with vertices `(±s/2, ±period/2)` and `(±period/2, ±s/2)`. Center the representative square at `(period/2, period/2)`, rotate it 45 degrees, and use side length `s`. Derive all seams through the common compiler; do not hard-code CSR.

- [ ] **Step 4: Run preset and scale tests**

Run: `cargo test --locked --lib sim::tiling::presets && cargo test --locked --lib sim::tiling::coverage`

Expected: all preset tests pass at multiple positive finite scales.

- [ ] **Step 5: Commit**

```bash
git add src/sim/tiling/presets.rs src/sim/tiling/mod.rs
git commit -m "feat: add editable periodic tiling presets"
```

### Task 7: Persistence, migration, and atomic Apply integration

**Files:**
- Modify: `src/sim/experiment.rs`
- Modify: `src/sim/experiment_model.rs`
- Modify: `src/sim/service.rs`
- Modify: `src/remote.rs`
- Test: `src/sim/experiment.rs`
- Test: `src/sim/service.rs`
- Test: `src/remote.rs`

**Interfaces:**
- Adds: `TileAddress { lattice_cell: [i32; 2], tile: TileId }` with `TileAddress::origin(tile)`, `CompiledDomain { addresses, address_to_index, offsets, neighbors }`, `GeometrySpec::PeriodicTiling { tiling: PeriodicTilingDraft, domain: TileDomainSpec, boundary: BoundarySpec }`, and format version 3.
- Consumes: tiling compiler and weight banks.
- Produces: geometry diagnostics with prototype/tile/edge paths and normalized compiled metadata in Apply responses.

- [ ] **Step 1: Add version-2 migration and rejected-geometry atomicity tests**

```rust
#[test]
fn version_two_grid_migrates_without_changing_cells_or_rule() {
    let before = version_two_two_channel_fixture();
    let migrated = decode_experiment_model(&encode_v2(&before)).unwrap();
    assert!(matches!(migrated.geometry, GeometrySpec::RasterGrid(_)));
    assert_eq!(migrated.channels, before.channels);
}

#[test]
fn overlap_rejection_preserves_active_runtime() {
    let mut service = service_fixture();
    let before = service.audit_snapshot();
    let result = service.apply(request_with_geometry(overlapping_tiling_fixture()));
    assert!(result.unwrap_err().diagnostics.iter().any(|d| d.code == "coverage_overlap"));
    assert_eq!(service.audit_snapshot(), before);
}
```

- [ ] **Step 2: Run persistence/service tests before integration**

Run: `cargo test --locked --lib sim::experiment::tests::version_two_grid_migrates_without_changing_cells_or_rule && cargo test --locked --lib sim::service::tests::overlap_rejection_preserves_active_runtime`

Expected: tests fail because version 3 and geometry variants are absent.

- [ ] **Step 3: Add version-3 wire model and diagnostics**

Preserve version-2 raster models exactly. `TileDomainSpec::Rect` activates every representative in each repeated patch; `Mask` stores `width * height * representative_count` booleans in patch-major/tile-ID order; `Sparse` stores sorted unique `TileAddress` values. Compile active addresses in lattice-cell then TileId order and expose that order as the only channel-plane/dense-runtime ordering. Serialize owned tiling/domain types and kernel variants. During Apply, run polygon, half-edge, coverage, mesh, domain expansion, and weight-bank compilation before backend allocation. Resolve boundary neighbors by lattice cell plus representative TileId, and attach diagnostics to `Tiling.translation_a`, `Tiling.translation_b`, `Tiling.Prototypes[id]`, `Tiling.Tiles[id].edge[n]`, `World.domain`, or `Kernels[id]` as appropriate.

- [ ] **Step 4: Bump and round-trip remote protocol metadata**

Increment `PROTOCOL_VERSION` once because complete drafts and normalized experiment metadata now contain tiling and kernel variants. Keep frame bounds and trailing-byte rejection.

- [ ] **Step 5: Run model, service, protocol, and no-default tests**

Run: `cargo test --locked --no-default-features --lib sim::experiment && cargo test --locked --no-default-features --lib sim::service && cargo test --locked --no-default-features --lib sim::tiling && cargo test --locked --no-default-features --lib remote`

Expected: all focused tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/sim/experiment.rs src/sim/experiment_model.rs src/sim/service.rs src/remote.rs
git commit -m "feat: persist and apply periodic polygon tilings"
```

### Task 8: Geometry regression corpus and fuzz-safe limits

**Files:**
- Create: `tests/fixtures/tilings/square.ron`
- Create: `tests/fixtures/tilings/hex_cells.ron`
- Create: `tests/fixtures/tilings/honeycomb.ron`
- Create: `tests/fixtures/tilings/octagon_square.ron`
- Create: `tests/fixtures/tilings/invalid_overlap.ron`
- Create: `tests/fixtures/tilings/invalid_gap.ron`
- Modify: `tests/workflow_contract.rs`

**Interfaces:** Provides stable human-readable fixtures used by UI and E2E plans.

- [ ] **Step 1: Save canonical preset fixtures and explicit invalid fixtures**

Generate them once through `save_experiment_model`, review the RON, then check in the stable text. Tests must load the checked-in files, not regenerate expected values at runtime.

- [ ] **Step 2: Add fixture contract tests**

```rust
#[test]
fn valid_tiling_fixture_corpus_compiles() {
    for name in ["square", "hex_cells", "honeycomb", "octagon_square"] {
        let model = load_fixture(name).unwrap();
        compile_experiment(&model).unwrap();
    }
}

#[test]
fn invalid_fixture_codes_are_stable() {
    assert_fixture_error("invalid_overlap", "coverage_overlap");
    assert_fixture_error("invalid_gap", "coverage_gap");
}
```

- [ ] **Step 3: Run fixture and hostile-size tests**

Run: `cargo test --locked --test workflow_contract tiling_fixture`

Expected: all corpus tests pass and unsafe candidate-copy/edge counts return diagnostics rather than allocating unbounded memory.

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures/tilings tests/workflow_contract.rs
git commit -m "test: add periodic tiling regression corpus"
```

### Task 9: Tiling completion gate

**Files:** None.

**Interfaces:** Produces the reviewed headless geometry prerequisite for the visual Workbench plan.

- [ ] **Step 1: Run complete local verification**

Run: `cargo fmt --check && cargo test --locked --no-default-features && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && cargo check --locked --target aarch64-unknown-linux-gnu && git diff --check`

Expected: every command exits zero; the cross-target check proves compilation, not ARM64 performance.

- [ ] **Step 2: Run sparse CUDA correctness on tinker**

Run on tinker: `cargo test --locked sim::cuda::tests::generic_csr_topology_step_matches_cpu_reference -- --exact`

Expected: CUDA executes on NVIDIA and matches the CPU reference.

- [ ] **Step 3: Request code review**

Use `superpowers:requesting-code-review`; resolve all Critical and Important geometry robustness, allocation-bound, migration, CSR, and CPU/CUDA findings before exposing the editor.
