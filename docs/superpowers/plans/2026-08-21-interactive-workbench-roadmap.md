# Interactive Experiment Workbench Roadmap

**Approved spec:** `docs/superpowers/specs/2026-08-21-interactive-experiment-workbench-design.md`

The feature is split into four sequential implementation plans because each
subsystem has a distinct correctness boundary and must leave Cellarium working
before the next begins.

## Execution order

1. [`2026-08-21-workbench-runtime-foundation.md`](2026-08-21-workbench-runtime-foundation.md)
   introduces stable experiment IDs, multi-channel/multi-kernel execution,
   versioned migration, atomic background Apply, and revision-aware C/S
   transport. Its milestone is a headless but fully authoritative runtime with
   all classic UI behavior preserved.
2. [`2026-08-21-growth-language-runtime.md`](2026-08-21-growth-language-runtime.md)
   adds the restricted Rust-like Growth language, typed CPU/CUDA execution,
   numeric diagnostics, traces, and plot sampling. Its milestone is backend
   parity for structured multi-input programs without exposing incomplete UI.
3. [`2026-08-21-periodic-polygon-tiling.md`](2026-08-21-periodic-polygon-tiling.md)
   adds robust periodic polygon geometry, mixed tiling presets, half-edge and
   coverage validation, CSR compilation, and area-aware kernels. Its milestone
   is headless compilation of square, hexagonal, honeycomb, and octagon-square
   fixtures on CPU/CUDA.
4. [`2026-08-21-visual-workbench-and-remote-e2e.md`](2026-08-21-visual-workbench-and-remote-e2e.md)
   exposes the complete outline-first editor, responsive footer, channel and
   polygon rendering, Growth plots, persistence UI, remote subscriptions, and
   hybrid PTY/E2E verification on tinker.

## Handoff gates

Every plan ends with formatting, CPU-only tests, default-feature tests, clippy,
diff checks, and a code-review gate. CUDA correctness and performance-sensitive
E2E run on tinker. A later plan may start only after all Critical and Important
findings in its prerequisite are resolved.

The plans intentionally use multiple small commits. They must be executed in an
isolated worktree and may be checkpointed between plans without exposing UI for
unsupported draft operations.
