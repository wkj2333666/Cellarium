# Basis Runtime, Agentic Validation, and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute basis-aware multi-channel/multi-kernel experiments on CPU and CUDA, synchronize them authoritatively, prove the complete UX through visual Agent journeys, and publish verified cross-architecture binaries.

**Architecture:** Preserve the optimized raster runtime as one `CompiledExperiment` variant and add a basis-sparse variant whose state is channel-major with basis-contiguous lattice sites. Compile periodic kernel planes into deterministic sparse weight rows and consume the same typed growth AST on CPU/CUDA. Use draft GitHub Releases for final ARM64 visual validation before publishing the exact tested assets.

**Tech Stack:** Rust 2024, existing CPU/CUDA backends and NVRTC codegen, protocol v9, GitHub Actions/Releases, Xvfb/Openbox/Kitty visual harness.

**Spec:** `docs/superpowers/specs/2026-08-23-basis-aware-workbench-agentic-validation-design.md`

## Global Constraints

- Never build Cellarium on the Raspberry Pi. It downloads and verifies draft/final GitHub Release ARM64 assets only.
- Compile, test, and benchmark simulation code on tinker; CUDA acceptance requires tinker's NVIDIA backend.
- Preserve the raster fast path and direct rendering.
- Server acknowledgement and authoritative revision prove Apply; optimistic client state does not.
- Distinguish server simulation, snapshot receive, UI draw, fresh graphics, Kitty consume, input-to-ack, and input-to-visible metrics.
- Do not publish a release until the exact draft-release assets pass the complete visual Agent journey and cleanup audit.

---

### Task 1: Basis-contiguous state layout and sparse CPU runtime

**Files:**
- Create: `src/sim/basis_runtime.rs`
- Modify: `src/sim/runtime.rs`
- Modify: `src/sim/world.rs`
- Modify: `src/sim/cpu.rs`
- Modify: `src/sim/service.rs`

**Interfaces:**
- Produces: `StateLayout { width, height, bases, channels }` and `index(channel, lattice_x, lattice_y, basis) -> usize`.
- Produces: `CompiledBasisKernel { row_offsets, lattice_offsets, source_bases, weights }`.
- Produces: `CompiledExperiment::{RasterFastPath, BasisSparse(CompiledBasisExperiment)}`.

- [ ] **Step 1: Write failing index and numerical-step tests**

```rust
#[test]
fn state_is_channel_major_and_basis_contiguous() {
    let l = StateLayout::new(2, 2, 2, 3).unwrap();
    assert_eq!(l.index(0, 0, 0, 0), 0);
    assert_eq!(l.index(0, 0, 0, 1), 1);
    assert_eq!(l.index(1, 0, 0, 0), 8);
}
```

Create a two-basis, one-channel, two-kernel fixture and hand-compute one periodic CPU step including source-basis weights and two independent growth programs.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib sim::basis_runtime sim::runtime::tests::two_basis_sparse_step`

- [ ] **Step 3: Implement checked layout and sparse compilation**

Use checked multiplication for `width * height * bases * channels`. For each `(target_basis, output_channel, kernel)` compile sorted entries `(dy, dx, source_basis, weight)`, apply mask and normalization once, and store row offsets by target RuleSet/kernel. Reject duplicate or non-finite entries before allocation.

- [ ] **Step 4: Implement CPU stepping and preserve RasterFastPath**

For each lattice site, target basis, and output channel, gather every kernel potential using the boundary resolver, evaluate that binding's growth AST with its exact stable symbols and parameters, then apply its update mode. Swap the complete output buffer only if all results are finite.

- [ ] **Step 5: Run CPU parity and regression tests**

Run: `cargo test --no-default-features --lib sim::basis_runtime sim::runtime sim::cpu sim::service`

Expected: basis fixtures pass and legacy Conway/Lenia step results are byte-for-byte unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/sim/basis_runtime.rs src/sim/runtime.rs src/sim/world.rs src/sim/cpu.rs src/sim/service.rs
git commit -m "feat: execute basis-aware experiments on cpu"
```

### Task 2: CUDA basis-sparse execution and parity

**Files:**
- Modify: `src/sim/cuda.rs`
- Modify: `src/sim/cuda_codegen.rs`
- Modify: `src/sim/backend.rs`
- Test: inline CUDA code-generation tests and ignored tinker GPU parity test in `tests/remote_e2e.rs`

**Interfaces:**
- Consumes: `StateLayout` and `CompiledBasisKernel` from Task 1.
- Produces: one generated CUDA step kernel specialized by basis/channel/RuleSet counts and bounded sparse tables.

- [ ] **Step 1: Write failing CUDA source-structure tests**

Assert generated source indexes `channel → lattice site → basis`, embeds or uploads deterministic sparse row tables, binds each RuleSet's exact kernel symbols, and guards every intermediate/final value with the existing non-finite error flag.

- [ ] **Step 2: Run codegen RED**

Run: `cargo test --lib sim::cuda_codegen::tests::basis_sparse_codegen`

- [ ] **Step 3: Implement basis-sparse code generation and buffers**

Reuse the typed growth AST emitter. Upload row offsets, packed `(dx,dy,source_basis)`, weights, binding tables, and parameters once per compiled experiment. Keep RasterFastPath on its current optimized kernel.

- [ ] **Step 4: Run codegen GREEN and real GPU parity**

Run: `cargo test --lib sim::cuda_codegen sim::cuda`

Run on tinker GPU: `cargo test --release --test remote_e2e basis_cpu_cuda_parity -- --ignored --exact --nocapture`

Expected: CPU/CUDA outputs for square, hexagon, octagon-square, T-junction, three-channel RGB, two-basis independent RuleSets, and two-kernel growth agree within the existing backend tolerance; non-finite fixtures reject the whole step.

- [ ] **Step 5: Commit**

```bash
git add src/sim/cuda.rs src/sim/cuda_codegen.rs src/sim/backend.rs tests/remote_e2e.rs
git commit -m "feat: execute basis-aware experiments on cuda"
```

### Task 3: Authoritative C/S synchronization and correlated metrics

**Files:**
- Modify: `src/remote.rs`
- Modify: `src/app.rs`
- Create: `src/render/presentation_metrics.rs`
- Modify: `src/render/mod.rs`
- Modify: `src/tui/workbench/mod.rs`
- Modify: `tests/support/remote_probe.rs`
- Modify: `tests/support/terminal_probe.rs`
- Modify: `tests/remote_e2e.rs`

**Interfaces:**
- Produces: `PresentationMetrics` with independent fixed-window meters for snapshot receive, UI draw, fresh RGBA generation, and Kitty presentation.
- Produces: `InteractionCorrelation { ui_sequence, server_input_sequence, apply_request_id, acknowledged_revision, visible_generation }` in optional test telemetry.

- [ ] **Step 1: Write failing authoritative mirror tests**

Apply a two-basis/two-kernel spec, receive ApplyAccepted and ExperimentState, and assert selected basis/channel/RuleSet/kernel, source, parameters, colors, tiling arrangement metadata, and revision all match the server. Assert a dirty conflicting draft preserves its old base revision and cannot silently overwrite a newer server model.

- [ ] **Step 2: Write failing metrics tests with idle tails**

Feed 21 events during the first second of a three-second window and none later; assert the reported rate is `7 Hz`, not `20 Hz`. Re-draw one generation 100 times and assert UI draw increments while fresh and consume remain one.

- [ ] **Step 3: Run RED**

Run: `cargo test --lib remote app::tests::basis_authoritative_mirror render::presentation_metrics`

- [ ] **Step 4: Implement atomic mirror and metrics**

Validate the complete received model before replacing authoritative metadata. Update selection only when its stable ID vanished. Separate local draft receipts from server input acknowledgements; Apply correlation ends only when its request ID, accepted revision, authoritative snapshot, and resulting visible generation have all been observed.

- [ ] **Step 5: Run protocol and C/S tests**

Run: `cargo test --lib && cargo test --test remote_e2e -- --skip tinker && cargo test --test workbench_e2e`

- [ ] **Step 6: Commit**

```bash
git add src/remote.rs src/app.rs src/render src/tui tests
git commit -m "fix: correlate basis edits with authoritative frames"
```

### Task 4: Release workflow staging and architecture gates

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `tests/workflow_contract.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `README.md`

**Interfaces:**
- Version target: `0.2.0` after release-candidate validation.
- A tag build creates a **draft** GitHub Release containing all six architecture archives plus `SHA256SUMS`; publication is a separate explicit step after external Agent evidence.

- [ ] **Step 1: Write failing workflow contract tests**

Assert Linux x86_64/aarch64 artifacts include CPU+CUDA dynamic-loading support, macOS and Windows use CPU-only builds, checksum generation covers every archive, release creation uses `--draft`, and no workflow claims the Raspberry Pi built or benchmarked Cellarium.

- [ ] **Step 2: Run RED**

Run: `cargo test --test workflow_contract`

- [ ] **Step 3: Implement CI and draft-release staging**

Keep tag/package version equality. Add all unit/integration/no-default-feature gates before packaging. Create the release with:

```bash
gh release create "$GITHUB_REF_NAME" cellarium-* SHA256SUMS \
  --verify-tag --generate-notes --draft
```

Do not add an automatic publish job.

- [ ] **Step 4: Document user-visible Workbench and test boundaries**

Document direct mode, `cellarium connect`, Workbench discovery, basis/channel distinction, default inheritance, kernel gestures, growth signature/plots, Kitty preference, half-block fallback, and why virtual-X11 rates are not performance evidence.

- [ ] **Step 5: Run all remote pre-release gates**

Run on tinker: `cargo fmt --check && cargo test --locked --all-targets && cargo test --locked --no-default-features --all-targets && cargo clippy --locked --all-targets -- -D warnings && git diff --check`

- [ ] **Step 6: Commit**

```bash
git add .github/workflows Cargo.toml Cargo.lock README.md tests/workflow_contract.rs
git commit -m "ci: stage verified basis workbench releases"
```

### Task 5: Complete visual Agent journey and defect closure loop

**Files:**
- Modify: `tests/agentic/full-journey.md`
- Create per run: ignored evidence under `target/agentic/<run-id>/`
- Modify product/test files only through one regression-backed correction at a time.

**Interfaces:**
- Consumes: harness commands from `2026-08-23-visual-agentic-harness.md`.
- Produces: one evidence row and before/after screenshots for every required journey step, plus exact release and server identities.

- [ ] **Step 1: Commit the complete visual journey definition**

Write `tests/agentic/full-journey.md` as the exact ordered checklist from the specification, including required evidence fields and pass/fail semantics for every action. Commit it before producing a candidate so the tested journey is versioned with the candidate.

```bash
git add tests/agentic/full-journey.md docs/agentic-testing.md
git commit -m "test: define complete basis workbench journey"
```

- [ ] **Step 2: Publish the first draft release candidate through CI**

Set an RC package version such as `0.2.0-rc.1`, commit it, tag the exact commit, push, wait for all GitHub Actions jobs, and verify the draft Release contains six archives and valid `SHA256SUMS`. Do not build locally.

- [ ] **Step 3: Install matching tinker server and start a clean Pi session**

Download and verify the draft ARM64 asset on Pi. Download and verify the matching Linux x86_64 asset on tinker, install its single binary to `/home/wkj/.local/bin/cellarium`, and start a uniquely identified server session. Record both hashes and versions before opening Kitty.

- [ ] **Step 4: Execute the entire visual journey adaptively**

For each step in the spec's Required user journeys: capture, visually locate controls, act with real X11 events, await the applicable receipt/revision and a correlated new visual generation, capture, visually inspect, and record. Include non-axis-aligned polygon draw, confirmed T-junction, hexagon and octagon-square adjacency, three-channel RGB/custom color, basis selection, float wheel/fine/coarse/exact kernel edits, second kernel arity, copy-on-write/default relink, multiline growth editing/error repair/curve/2D heatmap, Apply, graphics deletion, resize/reconnect/stress, and the complete half-block journey.

Run a separate direct-mode journey in a fresh Kitty window using `kitten ssh tinker /home/wkj/.local/bin/cellarium`. Verify pause, paint, Workbench entry/exit, high-resolution graphics, input latency, and placement cleanup through the original SSH rendering path.

- [ ] **Step 5: Close every discovered blocker through an evidence-driven loop**

For each defect, use its evidence ID as follows: preserve the failing screenshots and exact action sequence; add the smallest remote unit/integration regression that reproduces the underlying state error; verify RED on tinker; implement one root-cause correction; verify GREEN plus adjacent suites; commit; build the next CI draft RC; download that RC on Pi; restart the visual journey from its first action. A changed hash, trace line, or error decoration cannot close a visual defect.

- [ ] **Step 6: Pass sustained responsiveness and cleanup**

Run at least ten minutes of mixed navigation, pan, zoom, paint, numeric editing, source editing, Apply/Revert, resize, and reconnect. Require bounded input-to-visible latency, no frozen window, no coordinate drift, no stale/cropped layer, and no growing image/process/shared-memory count. Stop by recorded identities and verify zero leftovers locally and on tinker.

- [ ] **Step 7: Run final draft `0.2.0` journey**

After the last RC passes, change only the package version to `0.2.0`, tag it, let CI create the draft release, download those exact final assets, repeat the full Kitty and half-block journeys, and attach the final report to the release notes. If anything fails, leave the release draft and return to Step 4 with a new patch version candidate.

- [ ] **Step 8: Commit the release evidence index**

```bash
git add docs/agentic-testing.md
git commit -m "test: index certified basis workbench evidence"
```

### Task 6: Publish and post-publication smoke

**Files:**
- No product changes unless the smoke produces a new failing regression; in that case the draft remains unpublished and Task 5 resumes.

- [ ] **Step 1: Verify the exact draft one last time**

Check tag → commit → Cargo version, six archive hashes, final visual report release identities, tinker GPU backend, direct raster smoke, C/S basis smoke, Kitty cleanup, half-block interaction, and clean process audits.

- [ ] **Step 2: Publish the tested draft**

Run: `gh release edit v0.2.0 --draft=false --latest`

Expected: the existing draft becomes the public release without rebuilding or replacing assets.

- [ ] **Step 3: Download the now-public ARM64 asset and run a short smoke**

Verify the public checksum, start a new clean Xvfb/Kitty session, discover Workbench, edit one kernel float, edit one valid growth line, Apply, return to Simulation, confirm the editor graphic disappears, and stop with zero leaks.

- [ ] **Step 4: Record completion**

Append the public release URL, final tag/commit/assets, smoke evidence paths, and zero-leak audit to the final report. Do not report Raspberry Pi virtual-display frame rate as product performance.

Expected final boundary: the exact published binaries have passed remote automated gates, CUDA parity, direct compatibility, full adaptive visual Agent testing, half-block interaction, stress, and post-publication smoke.

