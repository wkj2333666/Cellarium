# Configurable Kernels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement PLAN.md phase P5 so kernels are data-driven, inspectable, editable, loadable, and executable on CPU and CUDA without Rust source changes.

**Architecture:** Separate editable kernel definitions and safe expression evaluation from immutable backend-ready kernel arrays. CPU and CUDA both consume validated rectangular stencils and skip masked entries. The application owns a catalog and regeneration lifecycle while the TUI remains a presentation/input shell.

**Tech Stack:** Rust 2024, serde + RON, Ratatui/Crossterm, cudarc/NVRTC.

**Spec:** `docs/superpowers/specs/2026-08-20-configurable-kernels-design.md`

## Global Constraints

- Preserve the backend boundary: no CUDA type may appear in `App`, TUI, or core world state.
- Kernel dimensions are positive and no axis exceeds 129 cells.
- Explicit values and masks must contain exactly `width * height` entries.
- All expression values, parameters, generated values, and normalization denominators must be finite.
- `Normalization::Sum` divides all unmasked values by their finite sum when the absolute sum exceeds `1e-12`.
- CPU and CUDA must agree within `1e-5` on all parity fixtures.
- Invalid edited definitions must not replace the previous active kernel.
- Full gate: `cargo fmt -- --check && cargo test && cargo clippy --all-targets -- -D warnings && cargo build --release && python3 /tmp/cellarium_smoke.py`.

---

### Task 1: Safe Kernel Expression AST

**Files:**
- Create: `src/sim/expression.rs`
- Modify: `src/sim/mod.rs`

**Interfaces:**
- Produces: `pub enum KernelExpression`, `pub enum BinaryOp`, `pub enum UnaryOp`, `pub enum ExpressionVariable`, and `pub fn evaluate(expression: &KernelExpression, context: &ExpressionContext) -> Result<f32, KernelExpressionError>`.
- Produces: `pub struct ExpressionContext<'a> { pub x: f32, pub y: f32, pub radius: f32, pub distance: f32, pub parameters: &'a BTreeMap<String, f32> }`.

- [ ] **Step 1: Write failing evaluator tests**

Add tests to `src/sim/expression.rs`:

```rust
#[test]
fn evaluates_geometry_parameters_and_math() {
    let mut parameters = BTreeMap::new();
    parameters.insert("center".to_string(), 0.5);
    parameters.insert("width".to_string(), 0.25);
    let context = ExpressionContext {
        x: 3.0,
        y: -4.0,
        radius: 5.0,
        distance: 1.0,
        parameters: &parameters,
    };
    let expression = KernelExpression::Exp(Binary {
        op: BinaryOp::Div,
        lhs: Box::new(Binary {
            op: BinaryOp::Sub,
            lhs: Box::new(Variable(ExpressionVariable::Distance)),
            rhs: Box::new(Parameter("center".into())),
        }),
        rhs: Box::new(Parameter("width".into())),
    });

    assert_eq!(evaluate(&expression, &context).unwrap(), (1.0_f32).exp());
}

#[test]
fn rejects_missing_parameters_and_non_finite_results() {
    let parameters = BTreeMap::new();
    let context = ExpressionContext { x: 0.0, y: 0.0, radius: 1.0, distance: 0.0, parameters: &parameters };
    let missing = Parameter("missing".into());
    assert!(evaluate(&missing, &context).is_err());

    let overflow = Binary {
        op: BinaryOp::Mul,
        lhs: Box::new(Constant(f32::MAX)),
        rhs: Box::new(Constant(f32::MAX)),
    };
    assert!(evaluate(&overflow, &context).is_err());
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test sim::expression -- --nocapture`

Expected: compilation fails because `sim::expression` does not exist.

- [ ] **Step 3: Implement the minimal evaluator**

Implement serde-derivable AST variants for constants, named parameters, geometry variables, unary operations, and binary operations. Recursively evaluate and reject missing names, divide-by-zero, negative square roots, and non-finite results.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test sim::expression -- --nocapture`

Expected: all expression tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim/mod.rs src/sim/expression.rs
git commit -m "Add safe kernel expression evaluator"
```

### Task 2: Validated Kernel Model and Presets

**Files:**
- Create: `src/sim/kernel.rs`
- Modify: `src/sim/mod.rs`
- Modify: `src/sim/rule.rs`

**Interfaces:**
- Consumes: `KernelExpression` and evaluator from Task 1.
- Produces:
  - `pub enum Normalization { None, Sum }`
  - `pub enum KernelValues { Explicit(Vec<f32>), Expression(KernelExpression) }`
  - `pub struct KernelDefinition { name, width, height, anchor_x, anchor_y, mask, normalization, parameters, values }`
  - `pub struct Kernel { name, width, height, anchor_x, anchor_y, mask, normalization, parameters, values }`
  - `impl TryFrom<KernelDefinition> for Kernel`
  - `pub fn ring_definition(radius: usize, center: f32, width: f32) -> KernelDefinition`
  - `pub fn render_definition(width: usize, height: usize) -> KernelDefinition`
- Updates `SimulationSpec.kernel` to use `Kernel`.

- [ ] **Step 1: Write failing model tests**

Cover dimensions, non-square dimensions, asymmetric anchor, mask, normalization, radius metadata, explicit values, generated values, and invalid dimensions/lengths/non-finite sums. Include a test that a 33×21 kernel with anchor `(16,10)` has radius 16 and contains 693 evaluated values.

- [ ] **Step 2: Verify RED**

Run: `cargo test sim::kernel -- --nocapture`

Expected: compilation fails because the kernel module and model do not exist.

- [ ] **Step 3: Implement validation and evaluation**

Evaluate each included cell using normalized `x`, `y`, and `r` geometry, reject non-finite values, apply the mask, normalize when requested, and construct the immutable kernel. Replace the old square-only `Kernel { radius, values }` and update Lenia to use `ring_definition(13, 0.5, 0.5).build()`.

- [ ] **Step 4: Verify GREEN and refactor rule tests**

Run: `cargo test sim::kernel sim::rule -- --nocapture`

Expected: model and rule tests pass; Conway still has no convolution kernel.

- [ ] **Step 5: Commit**

```bash
git add src/sim/mod.rs src/sim/kernel.rs src/sim/rule.rs
git commit -m "Add data-driven validated kernels"
```

### Task 3: RON Kernel Loading and Startup Selection

**Files:**
- Modify: `Cargo.toml`
- Create: `src/sim/kernel_file.rs`
- Modify: `src/sim/mod.rs`, `src/main.rs`, `src/app.rs`

**Interfaces:**
- Consumes: `KernelDefinition` from Task 2.
- Produces: `pub fn load_kernel(path: &Path) -> Result<KernelDefinition, KernelFileError>`.
- Produces: `pub fn run_with_kernel(kernel: KernelDefinition) -> std::io::Result<()>`.
- Produces minimal `--kernel <path>` parsing in `main`.

- [ ] **Step 1: Write failing loader and CLI tests**

Use temporary files under `std::env::temp_dir()` to test valid expression kernels, explicit-value kernels, malformed RON, and invalid model data. Add an app test proving a custom definition becomes the active selected kernel.

- [ ] **Step 2: Verify RED**

Run: `cargo test kernel_file -- --nocapture`

Expected: module and loader do not exist.

- [ ] **Step 3: Implement loading**

Add `serde`, `derive`, and `ron`. Deserialize a `KernelDefinition`, preserving file and validation error context. Parse `--kernel <path>` in `main`; load before entering raw mode; print a concise error and return status 1 on failure.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test kernel_file -- --nocapture`

Expected: all loader tests pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/sim/mod.rs src/sim/kernel_file.rs src/main.rs src/app.rs
git commit -m "Load configurable kernels from RON"
```

### Task 4: Rectangular and Masked CPU/CUDA Stencils

**Files:**
- Modify: `src/sim/cpu.rs`
- Modify: `src/sim/cuda.rs`

**Interfaces:**
- Consumes: `Kernel::{width, height, anchor_x, anchor_y, mask, values}`.
- Produces: CPU and CUDA convolution over arbitrary rectangular stencils with skipped masked entries.
- Extends CUDA source signature to include `kernel_width`, `kernel_height`, `kernel_anchor_x`, and `kernel_anchor_y`.

- [ ] **Step 1: Write failing backend fixtures**

Construct CPU parity helpers for:

```rust
fn non_square_kernel() -> KernelDefinition;
fn asymmetric_masked_kernel() -> KernelDefinition;
fn unnormalized_kernel() -> KernelDefinition;
```

Test one CPU step and, when CUDA is available, one CUDA step against CPU with tolerance `1e-5`.

- [ ] **Step 2: Verify RED**

Run: `cargo test sim::cpu sim::cuda -- --nocapture`

Expected: new fixtures fail or cannot compile under the old square-only API.

- [ ] **Step 3: Implement CPU traversal**

Iterate `0..kernel.height` and `0..kernel.width`, convert each coordinate through the anchor, skip masked entries, and accumulate against periodic `World::get`.

- [ ] **Step 4: Implement CUDA traversal and parity**

Pass rectangle dimensions and anchor arguments. Iterate actual kernel bounds in CUDA C++, skip masked entries, synchronize after device-to-host transfer, and retain the existing double-buffer swap.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test sim::cpu sim::cuda -- --nocapture`

Expected: all CPU fixtures pass and CUDA parity passes on the installed GPUs.

- [ ] **Step 5: Commit**

```bash
git add src/sim/cpu.rs src/sim/cuda.rs
git commit -m "Support rectangular masked kernels on CPU and CUDA"
```

### Task 5: Kernel Catalog, Editing, Regeneration, and Preview

**Files:**
- Modify: `src/input.rs`
- Modify: `src/app.rs`
- Modify: `src/tui/mod.rs`
- Test: embedded module tests in each modified file

**Interfaces:**
- Consumes: `KernelDefinition`, `Kernel`, and loader catalog from Tasks 2–3.
- Produces commands: `NextKernel`, `NextKernelParameter`, `IncreaseKernelParameter`, `DecreaseKernelParameter`, `RegenerateKernel`, `ToggleKernelPreview`.
- Produces app accessors for selected kernel name, dimensions, radius, normalization, selected parameter, and preview state.

- [ ] **Step 1: Write failing command and app tests**

Test `K`, `Tab`, `+`, `-`, `G`, and `V` translation. Test that parameter edits update a definition, `G` rebuilds the active backend, invalid definitions preserve the previous kernel and report an error, and preview state toggles independently from simulation state.

- [ ] **Step 2: Verify RED**

Run: `cargo test input app tui -- --lib -- --nocapture`

Expected: new commands and accessors do not exist.

- [ ] **Step 3: Implement catalog and regeneration**

Store definitions separately from `SimulationSpec`. Selection switches definitions; parameter edits are finite; regeneration calls `Kernel::try_from`, updates `SimulationSpec`, preserves backend kind, and recreates the backend only after validation succeeds.

- [ ] **Step 4: Implement TUI preview**

Add a bounded overlay with kernel metadata and a sampled compact preview. Include P5 command hints and show the selected parameter/value without breaking existing status information.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test input app tui -- --lib -- --nocapture`

Expected: all interaction and rendering tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/input.rs src/app.rs src/tui/mod.rs
git commit -m "Add interactive kernel editing and preview"
```

### Task 6: Full Verification, Review, and Integration

**Files:**
- No new production files.
- Modify only if review or verification identifies a defect.

**Interfaces:**
- Consumes all prior tasks.
- Produces a verified merge to `main`.

- [ ] **Step 1: Run full gate**

Run:

```bash
cargo fmt -- --check && cargo test && cargo clippy --all-targets -- -D warnings && cargo build --release && python3 /tmp/cellarium_smoke.py
```

Expected: all tests pass, Clippy reports no warnings, release build succeeds, and PTY checks are true.

- [ ] **Step 2: Inspect repository**

Run:

```bash
git status --short --branch
git diff --check
git diff --stat
```

Expected: only intentional P5 files changed and no whitespace errors.

- [ ] **Step 3: Independent review**

Request review of `main..HEAD` against the P5 spec and completion criteria. Fix all Critical/Important findings with TDD before integration.

- [ ] **Step 4: Merge and verify merged result**

After review approval, fast-forward `main` to the feature branch, rerun the full gate on `main`, and delete only the fully merged feature branch.

- [ ] **Step 5: Commit integration state**

If review requires fixes, commit them with focused messages. The final `main` commit must leave the worktree clean.
