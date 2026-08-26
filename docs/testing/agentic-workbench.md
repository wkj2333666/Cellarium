# Workbench agentic interaction journey

This journey is a real user-session gate, not a substitute for unit or PTY
tests. An agent observes the framebuffer, chooses coordinates from the current
image, sends real X11 keyboard and mouse events, captures the resulting image,
and records a semantic judgement for every action.

## Preconditions

- Use a stable GitHub Release binary. On ARM64, obtain it with
  `scripts/agentic/fetch-release.sh TAG OUTPUT_DIR`; this verifies
  `SHA256SUMS` before extraction.
- Do not build on the Raspberry Pi.
- The host needs Xvfb, Openbox, Kitty, xdotool, ffmpeg, jq, and an SSH alias for
  the Cellarium server.
- Use a fresh run ID. Each run has an isolated HOME, XDG data directory,
  display claim, process group, screenshots, logs, and JSONL evidence.

## Session controls

```bash
scripts/agentic-workbench-journey.sh start RUN_ID kitty RELEASE_DIR tinker 160 40
scripts/agentic-workbench-journey.sh capture RUN_ID before
scripts/agentic-workbench-journey.sh action RUN_ID click 500 300 1
scripts/agentic-workbench-journey.sh capture RUN_ID after
scripts/agentic-workbench-journey.sh record RUN_ID world-paint mouse \
  "paint a visible cell" BEFORE.png AFTER.png
scripts/agentic-workbench-journey.sh observe RUN_ID world-paint pass \
  "the painted cell appears under the pointer and no stale placement remains"
scripts/agentic-workbench-journey.sh finish RUN_ID pass "all checkpoints passed"
```

Use `kitty` for direct graphics and `half-block` for the fallback. The same
`action` command supports key, text, click, double-click, drag, wheel, and
resize. Coordinates must be chosen from the latest screenshot; fixed
coordinates copied from a different geometry are not valid visual evidence.

## Required semantic checkpoints

1. The initial simulation fills the available viewport without cropping.
2. Entering blank Tiling removes the old graphics placement.
3. A triangle can be drawn, its third vertex can be undone, and it can be
   closed; neighboring periodic cells are visibly consistent.
4. Applying a hexagonal tiling and running uses hexagonal simulation geometry.
5. An unapplied topology change labels Channels as a draft-initial preview.
6. RGB channels can be added and each Inspector row can be selected by mouse.
7. Empty, inactive, zero, and active kernel cells remain visible and
   selectable; wheel, exact value, pan, zoom, and support edits are observable.
8. Kernel deletion that would invalidate Growth is rejected without mutation.
9. The equality Growth example displays both isolated threshold markers.
10. Every toolbar action remains visible and clickable after narrow and wide
    resizes, and section transitions leave no stale graphics.
11. The interaction-critical path also succeeds in half-block mode.

Every action must have a before image, an after image, and an explicit visual
observation. A failed observation is recorded as a defect. A passing run is
refused while a defect is unresolved or an action lacks an observation.

## Cleanup and evidence

`finish` stops only the process group whose PID/start-time identity is stored
in the run manifest. If a journey aborts, call `stop` with its exact run ID.
Never kill Xvfb, Kitty, Openbox, or Cellarium by broad process name.

The run directory is under `target/agentic/RUN_ID` unless
`AGENTIC_TARGET_DIR` is set. It contains `frames/`, `logs/`,
`evidence.jsonl`, `report.md`, and `manifest.env`. Final release evidence
must name the stable tag, commit, ARM64 asset URL and SHA-256, tinker gate
commands, all screenshot paths, and cleanup status.
