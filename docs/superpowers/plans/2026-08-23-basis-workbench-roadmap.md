# Basis-aware Workbench Delivery Roadmap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved basis-aware visual Workbench through four independently reviewable plans.

**Architecture:** Establish the real-Kitty visual harness first, then introduce the model and periodic validator, build the editors against those stable interfaces, and finally add the basis runtime, authoritative C/S integration, exhaustive visual journey, and release. Every boundary leaves the existing direct/raster product working.

**Tech Stack:** Rust 2024, serde/RON, crossterm, ratatui, CPU RGBA rasterization, Kitty Graphics Protocol, CUDA, Bash, Xvfb, Openbox, xdotool, ffmpeg, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-08-23-basis-aware-workbench-agentic-validation-design.md`

## Global Constraints

- Never build, test, benchmark, or measure Cellarium simulation performance on the local ARM64 Raspberry Pi.
- Local C/S journeys must download a prebuilt GitHub Release ARM64 binary, verify `SHA256SUMS`, and record tag, URL, SHA-256, and version.
- Source builds and automated Rust tests run on tinker; simulation performance measurements use tinker's NVIDIA backend.
- Preserve direct simulation mode, C1 client/server mode, Kitty-by-default selection, and the interactive half-block fallback.
- Default channel count is exactly one; default kernel count per RuleSet is exactly one; additions are explicit.
- A visual agentic journey is required for release. PTY tests, protocol parsing, internal traces, and screenshot hashes cannot replace visual judgment.

---

## Plans

1. `2026-08-23-visual-agentic-harness.md` — isolated real-Kitty session, release acquisition, visual action/capture tools, and retained failing baseline.
2. `2026-08-23-basis-ruleset-periodic-model.md` — basis-aware RuleSets, copy-on-write defaults, robust periodic arrangement, T-junctions, budgets, fixtures, and wire round trips.
3. `2026-08-23-basis-workbench-visual-editors.md` — shared transforms and the Tiling, Kernel, Growth, Channel, shell, and fallback interactions.
4. `2026-08-23-basis-runtime-agentic-release.md` — CPU/CUDA execution, authoritative synchronization, metrics, complete visual journeys, cleanup, CI, and publication.

Execute these plans in order. Each plan ends with a focused review and a commit; do not begin the next plan while the preceding acceptance boundary is red.

