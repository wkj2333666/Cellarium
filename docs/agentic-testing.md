# Visual agentic testing

Cellarium's user-level acceptance test runs the released ARM64 client in a
real headless X11/Openbox/Kitty session. It does not build Cellarium on the
Raspberry Pi. The matching server and all performance work run on `tinker`.

## What is—and is not—the oracle

The final framebuffer is the primary oracle. An Agent must open each retained
PNG, understand the current layout, choose the next control from visible
pixels, and judge the visible result. Coordinates, image hashes, protocol
traces, acknowledgements, and shell exit codes may correlate an input with a
frame; none of them may declare that the interaction is usable or correct.

Every action follows this loop:

1. Capture the entire screen, including Kitty borders, footer, and pointer.
2. Visually locate the intended control in that current frame. Do not reuse a
   memorized coordinate from a previous layout or resolution.
3. Send one real X11 keyboard or mouse action with `action.sh`.
4. Where applicable, wait for the matching server acknowledgement or revision;
   this establishes causality, not visual success.
5. Capture the entire screen again.
6. Visually compare before and after, assess discoverability, feedback,
   precision, clipping, stale graphics, and responsiveness, then record an
   observation with `evidence.sh`.

An action without both images and a visual observation is incomplete. A run
with an unresolved defect cannot finish `pass`.

## Environment

Required lightweight packages are `xvfb`, `openbox`, `kitty`, `xdotool`,
`ffmpeg`, `jq`, `curl`, `file`, and `sha256sum`. No physical monitor, desktop
environment, display manager, GNOME, or KDE is involved.

`fetch-release.sh TAG OUTPUT_DIR` downloads
`cellarium-TAG-linux-aarch64.tar.gz` plus that release's `SHA256SUMS`, verifies
the archive before extraction, and records the immutable release identity.
An authenticated `gh` installation is used when available; public HTTPS is the
fallback. `--from-dir` is only for the offline verifier tests.

`session.sh start RUN_ID COLS ROWS -- COMMAND ARG...` creates one private
Xvfb/Openbox/Kitty process group. Its manifest records the display, process
identity, window, client, dimensions, and release metadata. `session.sh stop`
signals only that verified process group and audits its X socket and private
runtime cleanup.

`capture.sh` retains complete PNG frames. `action.sh` resolves the live Kitty
window before every action and rejects coordinates outside it. `evidence.sh`
stores JSON Lines plus `report.md`; it never interprets a screenshot.

## Required real-world modes

- Released ARM64 client: `cellarium connect tinker`, matching server installed
  at `/home/wkj/.local/bin/cellarium` on `tinker`.
- Direct rendering: `kitten ssh tinker /home/wkj/.local/bin/cellarium`.
- Kitty Graphics and forced half-block fallback, both fully interactive.

The C/S journey is the performance oracle. Direct and half-block journeys are
interaction and visual-correctness gates. All server processes must be started
with recorded PIDs and stopped by those identities.
