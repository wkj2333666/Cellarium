# Hybrid Remote End-to-End Implementation Plan

> Execute test-first. Do not use the local ARM64 host for simulation or
> rendering performance measurements.

## 1. Establish a trustworthy tinker deployment

- Build the current source natively on tinker with the CUDA feature.
- Probe strict CUDA construction and decode a real server snapshot.
- Atomically install only when the snapshot backend is NVIDIA/CUDA; retain a
  rollback binary and record the installed checksum.

## 2. Specify observable protocol performance

- Add failing protocol round-trip tests for completed step timing and
  authoritative input state transitions.
- Version the wire protocol when fields change; reject mismatches clearly.
- Populate snapshot performance only from completed server work.

## 3. Build the protocol E2E probe

- Add a reusable framed reader/writer and observation collector.
- Spawn the installed server through the product's SSH command path.
- Inject pause, step, reset/clear, and mouse edit messages.
- Produce a bounded JSON report containing real tick/snapshot cadence and
  input-to-state latency.

## 4. Emulate Kitty shared-memory consumption

- Add parser tests for split/coalesced Kitty APC commands.
- Add a test that opens, validates, and unlinks a real POSIX shm object named by
  a `t=s` command.
- Extend the PTY harness to keep consuming frames for the whole test and record
  completed consumption timestamps.

## 5. Build the full terminal E2E probe

- Run the actual `cellarium connect <alias>` command in a small PTY.
- Inject keyboard bytes and SGR mouse events.
- Correlate each state transition with the next consumed Kitty frame.
- Write complementary protocol and terminal JSON reports with auditable raw
  intervals.

## 6. Fix latency at the measured bottleneck

- Read protocol input independently and drain it before simulation work; run
  at most one completed simulation step between input drains, so an action is
  bounded by one in-flight step rather than an accumulated step batch.
- Preserve latest-snapshot backpressure instead of building an output queue.
- Keep simulation state mutation single-owner and cover the one-step scheduling
  bound with a regression test.

## 7. Correct UI metric semantics

- Show server step/simulation data from the server snapshot.
- Measure client render rate after `terminal.draw()` completes and record local
  draw duration separately.
- Label server simulation, snapshot receive, UI draw, and fresh graphics values
  so a target rate cannot be mistaken for observed throughput.

## 8. Verify and publish

- Run unit/integration tests with and without CUDA locally (correctness only).
- Run the hybrid E2E repeatedly against tinker and retain the JSON reports.
- Request code review, resolve findings, and rerun all relevant verification.
- Push the commit and publish the next release through CI after the real tinker
  chain meets the correctness and latency gates.
