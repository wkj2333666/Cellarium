# Workbench Runtime Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce the versioned multi-channel/multi-kernel experiment model, atomic Apply service, and revision-aware Direct/C/S transport while preserving all current single-channel behavior.

**Architecture:** Add a stable-ID experiment schema beside the legacy runtime, migrate legacy files into it, compile it into channel-major runtime state, and place construction behind an `ExperimentService` that swaps only fully built experiments. Remote Apply carries complete drafts and diagnostics; existing input sequence acknowledgements remain independent and intact.

**Tech Stack:** Rust 2024, serde/RON, existing CPU/CUDA backends, crossterm protocol framing, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-21-interactive-experiment-workbench-design.md`

## Global Constraints

- New experiments default to one channel and a runnable square grid.
- A growth signature has exactly one ordinary input per kernel targeting that channel; implicit `self` and parameters do not count.
- Frozen channels may be kernel sources but may not be kernel targets.
- Apply is build-then-swap; failure must preserve active state, tick, and revision.
- Direct and C/S modes use the same validator and runtime builder.
- Existing scalar experiments, Direct rendering, Kitty graphics, and protocol input acknowledgements remain functional.
- Do not measure simulation performance on the local ARM64 machine.

---

### Task 1: Stable-ID experiment model

**Files:**
- Create: `src/sim/experiment_model.rs`
- Modify: `src/sim/mod.rs`
- Test: `src/sim/experiment_model.rs`

**Interfaces:**
- Produces: `ChannelId(u32)`, `KernelId(u32)`, `ChannelSpec`, `ChannelDisplay`, `DisplayColor`, `RgbColor`, `KernelSlot`, `GrowthSource`, `UpdateMode`, `GeometrySpec`, `GridGeometry`, `ExperimentSpec`, `ExperimentModelError`.
- Produces: `ExperimentSpec::single_channel_lenia(width, height)`, `validate_structure(&ExperimentSpec) -> Result<(), Vec<ExperimentModelError>>`.

- [ ] **Step 1: Write failing model tests**

```rust
#[test]
fn default_model_is_single_channel_and_runnable() {
    let model = ExperimentSpec::single_channel_lenia(32, 24);
    assert_eq!(model.channels.len(), 1);
    assert_eq!(model.channels[0].initial.len(), 32 * 24);
    assert!(validate_structure(&model).is_ok());
}

#[test]
fn growth_inputs_are_exactly_targeting_kernels() {
    let mut model = ExperimentSpec::single_channel_lenia(4, 4);
    let channel = model.channels[0].id;
    model.kernels.push(KernelSlot::identity(
        KernelId(1), "crowd", channel, channel,
    ));
    model.growth[0].kernel_inputs.push(KernelId(999));
    let errors = validate_structure(&model).unwrap_err();
    assert!(errors.iter().any(|e| matches!(e,
        ExperimentModelError::GrowthKernelMismatch { target, .. } if *target == channel
    )));
}

#[test]
fn frozen_target_is_rejected_but_frozen_source_is_allowed() {
    let mut model = ExperimentSpec::single_channel_lenia(2, 2);
    let frozen = model.add_channel("environment", true);
    let active = model.channels[0].id;
    model.kernels.push(KernelSlot::identity(KernelId(7), "signal", frozen, active));
    assert!(validate_structure(&model).is_ok());
    model.kernels[0].target = frozen;
    assert!(validate_structure(&model).is_err());
}
```

- [ ] **Step 2: Run the focused tests and verify the missing module failure**

Run: `cargo test --locked --lib sim::experiment_model`

Expected: compilation fails because `sim::experiment_model` and its types do not exist.

- [ ] **Step 3: Add the model and structural validator**

Use newtype IDs and keep cross-references out of vector indices:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KernelId(pub u32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelSpec {
    pub id: ChannelId,
    pub name: String,
    pub frozen: bool,
    pub initial: Vec<f32>,
    pub boundary_constant: f32,
    pub display: ChannelDisplay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelDisplay {
    pub color: DisplayColor,
    pub visible: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayColor { Auto, Custom(RgbColor) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor { pub red: u8, pub green: u8, pub blue: u8 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelSlot {
    pub id: KernelId,
    pub symbol: String,
    pub name: String,
    pub source: ChannelId,
    pub target: ChannelId,
    pub definition: KernelDefinition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateMode { GrowthRate, DirectUpdate }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowthSource {
    pub target: ChannelId,
    pub kernel_inputs: Vec<KernelId>,
    pub parameters: BTreeMap<String, f32>,
    pub source: String,
    pub mode: UpdateMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GeometrySpec { RasterGrid(GridGeometry) }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridGeometry {
    pub width: u32,
    pub height: u32,
    pub boundary: BoundarySpec,
}
```

`validate_structure` must collect, not short-circuit, duplicate IDs/names/symbols, non-finite state/parameters, wrong state lengths, missing source/target IDs, frozen targets, duplicate or mismatched growth bindings, opacity outside `0..=1`, and invalid kernel definitions. Generate `kernel_inputs` in stable kernel-ID order in constructors; never derive bindings from vector position.

- [ ] **Step 4: Run focused tests**

Run: `cargo test --locked --lib sim::experiment_model`

Expected: all experiment-model tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim/experiment_model.rs src/sim/mod.rs
git commit -m "feat: add stable multi-channel experiment model"
```

### Task 2: Version-2 persistence and legacy migration

**Files:**
- Modify: `src/sim/experiment.rs`
- Modify: `src/sim/experiment_model.rs`
- Test: `src/sim/experiment.rs`
- Test: `tests/workflow_contract.rs`

**Interfaces:**
- Consumes: `ExperimentSpec`, IDs, channel/kernel/growth types from Task 1.
- Produces: `EXPERIMENT_FORMAT_VERSION = 2`, `ExperimentFileV2`, `load_experiment_model`, `save_experiment_model`.
- Preserves: current `load_experiment`/`save_experiment` compatibility wrappers until App migration in Task 7.

- [ ] **Step 1: Add migration and round-trip tests**

```rust
#[test]
fn version_one_lenia_migrates_to_one_channel_one_kernel() {
    let legacy = legacy_lenia_fixture_ron();
    let model = load_experiment_model_from_str(&legacy).unwrap();
    assert_eq!(model.channels.len(), 1);
    assert_eq!(model.kernels.len(), 1);
    assert_eq!(model.kernels[0].symbol, "potential");
    assert_eq!(model.growth[0].kernel_inputs, vec![model.kernels[0].id]);
}

#[test]
fn version_two_roundtrip_preserves_ids_routing_and_source() {
    let model = two_channel_fixture();
    let encoded = encode_experiment_model(&model).unwrap();
    assert_eq!(decode_experiment_model(&encoded).unwrap(), model);
}
```

The literal legacy fixture must be checked into the test body so the test does not serialize with current types before testing migration.

- [ ] **Step 2: Verify the tests fail against format version 1**

Run: `cargo test --locked --lib sim::experiment::tests::version_`

Expected: failure because version 2 APIs and migration are absent.

- [ ] **Step 3: Add version probing and pure migration**

Parse this header first:

```rust
#[derive(Deserialize)]
struct FormatProbe { format_version: u32 }
```

Dispatch version 0/1 to the existing wire shape, convert it to `ExperimentSpec`, and dispatch version 2 to `ExperimentFileV2`. Unknown newer versions return `UnsupportedVersion`. Migration rules are exact: one legacy world becomes `ChannelId(0)`, the legacy kernel becomes `KernelId(0)` with symbol `potential`, classic growth source is preserved, and absent topology becomes the default one-tile raster grid.

- [ ] **Step 4: Run persistence and workflow tests**

Run: `cargo test --locked --lib sim::experiment && cargo test --locked --test workflow_contract`

Expected: all persistence and workflow contract tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim/experiment.rs src/sim/experiment_model.rs tests/workflow_contract.rs
git commit -m "feat: migrate experiments to version two"
```

### Task 3: Channel-major runtime compiler and CPU executor

**Files:**
- Create: `src/sim/runtime.rs`
- Modify: `src/sim/world.rs`
- Modify: `src/sim/cpu.rs`
- Modify: `src/sim/mod.rs`
- Test: `src/sim/runtime.rs`
- Test: `src/sim/cpu.rs`

**Interfaces:**
- Produces: `CompiledKernelInput`, `CompiledChannelRule`, `CompiledExperiment`, `compile_experiment(&ExperimentSpec)`, `CpuExperimentBackend::step(&mut ChannelWorld)`.
- Consumes: existing expression parser/evaluator temporarily; Plan 2 replaces only the program body compiler.

- [ ] **Step 1: Add cross-channel and frozen-channel tests**

```rust
#[test]
fn two_targets_receive_only_their_routed_kernel_inputs() {
    let spec = routed_two_channel_fixture();
    let compiled = compile_experiment(&spec).unwrap();
    let mut world = ChannelWorld::from_channels(1, 1, &[vec![0.25], vec![0.75]]).unwrap();
    CpuExperimentBackend::new(compiled).step(&mut world).unwrap();
    assert!((world.get(0, 0, 0) - 0.75).abs() < 1e-6);
    assert!((world.get(1, 0, 0) - 0.25).abs() < 1e-6);
}

#[test]
fn frozen_channel_is_copied_without_update() {
    let (compiled, mut world) = frozen_source_fixture();
    let before = world.channel_cells(1).to_vec();
    CpuExperimentBackend::new(compiled).step(&mut world).unwrap();
    assert_eq!(world.channel_cells(1), before);
}

#[test]
fn constant_boundary_samples_each_source_channels_own_constant() {
    let (compiled, mut world) = two_channel_constant_boundary_fixture(0.25, 0.75);
    CpuExperimentBackend::new(compiled).step(&mut world).unwrap();
    assert_eq!(world.get(0, 0, 0), 0.25);
    assert_eq!(world.get(1, 0, 0), 0.75);
}
```

- [ ] **Step 2: Run tests and confirm the compiler/backend are missing**

Run: `cargo test --locked --no-default-features --lib sim::runtime && cargo test --locked --no-default-features --lib sim::cpu`

Expected: compilation failure for missing runtime types.

- [ ] **Step 3: Make `ChannelWorld` a complete public runtime state**

Add checked `from_channels`, `replace_all`, `cells`, `next_cells_mut`, and `discard_next`. Keep channel-major indexing. Do not remove `World`; legacy rendering remains until Task 7.

- [ ] **Step 4: Compile stable IDs to dense indices once**

`compile_experiment` builds `BTreeMap<ChannelId, usize>`, builds every kernel exactly once, parses each growth source with the existing expression parser, verifies its symbol set against its routed kernel symbols plus `self` and parameters, and returns:

```rust
pub struct CompiledChannelRule {
    pub target: usize,
    pub frozen: bool,
    pub mode: UpdateMode,
    pub inputs: Vec<CompiledKernelInput>,
    pub parameters: BTreeMap<String, f32>,
    pub update: KernelExpression,
}
```

The compiler resolves GridGeometry boundary policy plus every source channel's constant into a `CompiledBoundary`. Kernel sampling must implement Open/Constant, Periodic, Clamp, and Reflect explicitly instead of calling `ChannelWorld::get`, which remains a periodic compatibility accessor. The CPU step populates a fresh symbol map per tile, evaluates all targets from the old current buffer, writes every next channel, checks `is_finite`, then swaps exactly once. `GrowthRate` applies `clamp(self + dt * result, 0, 1)`; `DirectUpdate` applies `clamp(result, 0, 1)`.

- [ ] **Step 5: Run CPU and world tests**

Run: `cargo test --locked --no-default-features --lib sim::world && cargo test --locked --no-default-features --lib sim::runtime && cargo test --locked --no-default-features --lib sim::cpu`

Expected: all focused tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/sim/runtime.rs src/sim/world.rs src/sim/cpu.rs src/sim/mod.rs
git commit -m "feat: execute multi-channel experiment rules on CPU"
```

### Task 4: CUDA multi-target execution parity

**Files:**
- Modify: `src/sim/cuda_codegen.rs`
- Modify: `src/sim/cuda.rs`
- Modify: `src/sim/backend.rs`
- Modify: `src/sim/backend_error.rs`
- Test: `src/sim/cuda.rs`

**Interfaces:**
- Consumes: `CompiledExperiment` from Task 3.
- Produces: `CudaExperimentBackend::new(CompiledExperiment, tile_count)`, `ExperimentBackend::{Cpu,Cuda}`, `step_channels` with one atomic buffer swap.

- [ ] **Step 1: Add guarded CPU/CUDA parity tests**

```rust
#[test]
fn multi_target_cuda_matches_cpu_or_skips_without_driver() {
    let spec = routed_two_channel_fixture();
    let compiled = compile_experiment(&spec).unwrap();
    let mut cpu_world = two_channel_world();
    let mut gpu_world = two_channel_world();
    let Ok(mut gpu) = CudaExperimentBackend::new(compiled.clone(), 16) else { return };
    CpuExperimentBackend::new(compiled).step(&mut cpu_world).unwrap();
    gpu.step(&mut gpu_world).unwrap();
    for (lhs, rhs) in cpu_world.cells().iter().zip(gpu_world.cells()) {
        assert!((lhs - rhs).abs() < 1e-5, "{lhs} != {rhs}");
    }
}
```

- [ ] **Step 2: Run the parity test before implementation**

Run: `cargo test --locked --lib sim::cuda::tests::multi_target_cuda_matches_cpu_or_skips_without_driver`

Expected: compile failure because `CudaExperimentBackend` is missing.

- [ ] **Step 3: Generate one entry point per target channel**

Generate sanitized names from dense target indices, pass the common old-state buffer and a disjoint target slice in the next-state buffer, and embed only whitelisted expression AST operations. Generate the same Open/Constant, Periodic, Clamp, and Reflect source sampler used by the CPU semantics, including per-source-channel constants. Dispatch all target kernels on one stream, preserve frozen channels with device-to-device copies, set a device non-finite flag, synchronize once, and swap only when the flag is clear. Never write a target channel in place.

- [ ] **Step 4: Add backend selection without removing legacy selection**

Introduce `ExperimentBackend` beside `SimulationBackend`; construct it from `CompiledExperiment`. CPU-only builds return `CudaNotCompiled` for strict CUDA and otherwise select CPU. App migration in Task 7 removes the duplicate path after all compatibility tests pass.

- [ ] **Step 5: Run CPU-only, default-feature, and cross-target checks**

Run: `cargo test --locked --no-default-features --lib sim::backend && cargo test --locked --no-default-features --lib sim::runtime && cargo test --locked --lib sim::cuda && cargo test --locked --lib sim::backend`

Expected: all focused tests pass; CUDA tests return early only when no driver exists.

- [ ] **Step 6: Commit**

```bash
git add src/sim/cuda_codegen.rs src/sim/cuda.rs src/sim/backend.rs src/sim/backend_error.rs
git commit -m "feat: execute multi-channel experiment rules on CUDA"
```

### Task 5: Atomic experiment service

**Files:**
- Create: `src/sim/service.rs`
- Modify: `src/sim/mod.rs`
- Test: `src/sim/service.rs`

**Interfaces:**
- Produces: `ExperimentService`, `ActiveExperiment`, `PrepareJob`, `PreparedExperiment`, `ApplyRequest`, `ApplyAccepted`, `ApplyRejected`, `Diagnostic`, `DiagnosticPath`.
- Produces: `ExperimentService::begin_prepare(&self, request: ApplyRequest) -> Result<PrepareJob, ApplyRejected>`, `PrepareJob::build(self) -> Result<PreparedExperiment, ApplyRejected>`, `commit_prepared(&mut self, prepared: PreparedExperiment) -> Result<ApplyAccepted, ApplyRejected>`, `snapshot_active_experiment(&self) -> ExperimentSpec`, plus synchronous `apply` for Direct/tests.

- [ ] **Step 1: Add atomicity and revision tests**

```rust
#[test]
fn rejected_apply_preserves_runtime_tick_state_and_revision() {
    let mut service = service_fixture();
    service.step().unwrap();
    let before = service.audit_snapshot();
    let mut invalid = service.active_spec().clone();
    invalid.channels[0].initial[0] = f32::NAN;
    let rejected = service.apply(ApplyRequest {
        request_id: 9,
        base_revision: service.revision(),
        draft: invalid,
    });
    assert!(rejected.is_err());
    assert_eq!(service.audit_snapshot(), before);
}

#[test]
fn stale_revision_is_rejected_before_build() {
    let mut service = service_fixture();
    let result = service.apply(ApplyRequest {
        request_id: 10,
        base_revision: service.revision() + 1,
        draft: service.active_spec().clone(),
    });
    assert!(result.unwrap_err().diagnostics.iter().any(|d| d.code == "revision_conflict"));
}

#[test]
fn apply_jobs_and_candidates_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<PrepareJob>();
    assert_send::<PreparedExperiment>();
}

#[test]
fn active_snapshot_contains_current_channel_state() {
    let mut service = service_fixture();
    service.step().unwrap();
    let exported = service.snapshot_active_experiment();
    assert_eq!(exported.channels[0].initial, service.world().channel_cells(0));
}
```

- [ ] **Step 2: Run the tests and verify the service is missing**

Run: `cargo test --locked --no-default-features --lib sim::service`

Expected: compile failure for the missing service.

- [ ] **Step 3: Implement build-then-swap**

`begin_prepare` checks the base revision and returns an owned job containing the request and selected backend kind. `PrepareJob::build` validates model structure, compiles the model, allocates the selected backend, clones initial state into a candidate `ChannelWorld`, and executes one non-committing step without borrowing service ownership. `PreparedExperiment` must satisfy a compile-time `Send` assertion. `commit_prepared` rechecks `base_revision`, replaces `self.active`, increments revision with checked arithmetic, and returns the normalized spec. Convert every error into a stable diagnostic code and object/field path. The synchronous `apply` is exactly `begin_prepare`, `build`, then `commit_prepared`.

- [ ] **Step 4: Run service tests**

Run: `cargo test --locked --no-default-features --lib sim::service`

Expected: all service tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim/service.rs src/sim/mod.rs
git commit -m "feat: apply experiments atomically"
```

### Task 6: Revision-aware remote Apply protocol

**Files:**
- Modify: `src/remote.rs`
- Modify: `tests/remote_e2e.rs`
- Test: `src/remote.rs`

**Interfaces:**
- Consumes: Task 5 request/response and diagnostics.
- Produces new `RemoteMessage` variants: `ExperimentState { revision, normalized_experiment }`, `ApplyDraft(ApplyRequest)`, `ApplyAccepted(ApplyAccepted)`, `ApplyRejected(ApplyRejected)`.
- Preserves: `Input { sequence, .. }`, `Snapshot.applied_input_sequence`, latest-only snapshot queue.

- [ ] **Step 1: Add round-trip and malformed-frame tests**

```rust
#[test]
fn apply_messages_roundtrip_complete_drafts_and_paths() {
    let message = RemoteMessage::ApplyDraft(ApplyRequest {
        request_id: 44,
        base_revision: 7,
        draft: two_channel_fixture(),
    });
    assert_eq!(roundtrip(message.clone()), message);
}

#[test]
fn oversized_apply_draft_is_rejected_before_allocation() {
    let header = frame_header(PROTOCOL_VERSION, APPLY_DRAFT_TAG, MAX_FRAME_SIZE + 1);
    assert!(matches!(read_message(&mut &header[..]), Err(ProtocolError::Invalid(_))));
}

#[test]
fn steady_snapshot_does_not_repeat_full_experiment_source() {
    let snapshot = large_source_snapshot_fixture();
    let bytes = encode_for_test(RemoteMessage::Snapshot(snapshot)).unwrap();
    assert!(bytes.len() < 1024 + snapshot_cell_bytes());
}
```

- [ ] **Step 2: Run protocol tests before adding tags**

Run: `cargo test --locked --lib remote::tests`

Expected: compile failure for missing Apply variants.

- [ ] **Step 3: Bump the protocol and encode Apply messages**

Increment `PROTOCOL_VERSION` once. Use the existing bounded long-string RON encoding for the complete draft and diagnostics, assign explicit new tags, reject trailing bytes, and retain the 64 MiB frame cap. Send complete normalized experiment metadata in `ExperimentState` after Hello and in `ApplyAccepted`, not in every steady Snapshot. Snapshot carries revision, tick/rates/backend/error, current visual cells during this transitional plan, and applied input sequence. A client that observes an unknown revision requests/awaits `ExperimentState` before treating later metadata as authoritative.

- [ ] **Step 4: Run protocol and non-network E2E tests**

Run: `cargo test --locked --lib remote::tests && cargo test --locked --test remote_e2e -- --skip remote_protocol_e2e_on_tinker --skip remote_terminal_e2e_on_tinker`

Expected: all non-network tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/remote.rs tests/remote_e2e.rs
git commit -m "feat: carry atomic experiment apply over remote protocol"
```

### Task 7: Integrate the service into App, Direct, and server loops

**Files:**
- Modify: `src/app.rs`
- Modify: `src/main.rs`
- Modify: `src/tui/mod.rs`
- Modify: `tests/pty_startup.rs`
- Modify: `tests/workflow_contract.rs`

**Interfaces:**
- Consumes: `ExperimentService` and remote Apply messages.
- Produces: `App::experiment_service`, `App::submit_draft`, authoritative remote mirror revision/spec, legacy scalar getters backed by channel zero.

- [ ] **Step 1: Add compatibility and atomic server-loop tests**

```rust
#[test]
fn classic_lenia_still_starts_as_one_channel() {
    let app = App::new(SimulationSpec::lenia_orbium(), 32, 32);
    assert_eq!(app.channel_count(), 1);
    assert_eq!(app.active_revision(), 0);
}

#[test]
fn server_rejection_does_not_interrupt_snapshot_ticks() {
    let (mut harness, before) = running_server_harness();
    harness.send_invalid_apply();
    let rejected = harness.read_apply_rejected();
    let after = harness.read_snapshot_after(rejected.request_id);
    assert_eq!(after.revision, before.revision);
    assert!(after.tick >= before.tick);
}

#[test]
fn input_ack_progresses_while_apply_preparation_is_blocked() {
    let mut harness = server_harness_with_blocking_apply_builder();
    harness.send_valid_apply();
    harness.wait_until_apply_builder_blocks();
    let sequence = harness.send_pause();
    assert!(harness.read_snapshot_with_ack(sequence).paused);
    harness.release_apply_builder();
    assert!(harness.read_apply_accepted().revision > 0);
}
```

- [ ] **Step 2: Run focused App and PTY tests before migration**

Run: `cargo test --locked --lib app::tests::classic_lenia_still_starts_as_one_channel && cargo test --locked --lib app::tests::server_rejection_does_not_interrupt_snapshot_ticks && cargo test --locked --lib app::tests::input_ack_progresses_while_apply_preparation_is_blocked && cargo test --locked --test pty_startup`

Expected: new App tests fail because service-backed APIs are absent; existing PTY tests continue to pass.

- [ ] **Step 3: Route App construction and stepping through `ExperimentService`**

Keep `App::new(SimulationSpec, width, height)` as a compatibility constructor that creates a migrated one-channel `ExperimentSpec`. Replace direct backend/world ownership only after all old getters can delegate to the active service. An `ApplyCoordinator` permits one preparation in flight: the owner creates a `PrepareJob`, a worker owns `job.build()`, and the owner loop continues draining input and stepping/snapshotting; a second Apply receives an `apply_busy` rejection. The owner loop alone calls `commit_prepared` at a step boundary and sends the matching response. Add `fn assert_send<T: Send>() {}` and compile-time tests for both `PrepareJob` and `PreparedExperiment`; do not move Apply preparation back onto the input/simulation owner thread.

- [ ] **Step 4: Preserve classic TUI and CLI behavior**

Make channel zero available to the existing rasterizer until Plan 4 introduces channel compositing. Keep `--experiment`, `--save-experiment`, `server`, and `connect` syntax unchanged. Do not expose workbench controls yet.

- [ ] **Step 5: Run full local gates**

Run: `cargo fmt --check && cargo test --locked --no-default-features && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings`

Expected: all commands exit zero.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/main.rs src/tui/mod.rs tests/pty_startup.rs tests/workflow_contract.rs
git commit -m "refactor: run app through atomic experiment service"
```

### Task 8: Foundation remote regression gate

**Files:**
- Modify: `tests/support/remote_probe.rs`
- Modify: `tests/support/terminal_probe.rs`
- Modify: `scripts/e2e-tinker.sh`
- Modify: `docs/remote-viewer.md`

**Interfaces:**
- Consumes: protocol revision and Apply responses.
- Produces: protocol report fields `experiment_revision`, `apply_accept_ms`, `apply_reject_ms`; no new performance claim.

- [ ] **Step 1: Extend probe assertions**

Add a protocol probe sequence that submits an unchanged valid draft and then a structurally invalid draft. Require the valid response revision to increment, require the invalid response revision to remain unchanged, and require a later snapshot to carry each authoritative revision. Keep keyboard/mouse input-to-ack checks.

- [ ] **Step 2: Run non-network probe tests**

Run: `cargo test --locked --test remote_e2e -- --skip remote_protocol_e2e_on_tinker --skip remote_terminal_e2e_on_tinker`

Expected: all fake-stream and PTY-emulator tests pass.

- [ ] **Step 3: Run the tinker diagnostic gate only from a capable client**

Run: `CELLARIUM_E2E_HOST=tinker scripts/e2e-tinker.sh`

Expected: CUDA backend, continuing simulation ticks, valid Apply accepted, invalid Apply rejected, keyboard and mouse acknowledged, and Kitty shared-memory frames consumed. Record rates from tinker only.

- [ ] **Step 4: Commit**

```bash
git add tests/support/remote_probe.rs tests/support/terminal_probe.rs scripts/e2e-tinker.sh docs/remote-viewer.md
git commit -m "test: verify atomic remote experiment apply"
```

### Task 9: Foundation completion gate

**Files:** None.

**Interfaces:** Produces the reviewed prerequisite for the Growth Language plan.

- [ ] **Step 1: Run all local gates from a clean tree**

Run: `cargo fmt --check && cargo test --locked --no-default-features && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && git diff --check`

Expected: every command exits zero and no tracked diff remains except intentional plan tracking updates.

- [ ] **Step 2: Request code review**

Use `superpowers:requesting-code-review` against the range from the plan's first implementation commit through HEAD. Resolve all Critical and Important findings before beginning the Growth Language plan.
