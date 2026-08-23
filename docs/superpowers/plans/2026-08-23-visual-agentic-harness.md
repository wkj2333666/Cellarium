# Visual Agentic Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide a reproducible headless X11 environment in which an Agent sees final Kitty pixels and performs real keyboard and mouse actions against released Cellarium binaries.

**Architecture:** Shell tools own one isolated Xvfb/Openbox/Kitty process group and expose small start/action/capture/stop commands. The Agent—not the shell—interprets screenshots and chooses coordinates. A JSON-lines evidence log correlates visible actions with release identity and optional server acknowledgements without treating telemetry as the visual oracle.

**Tech Stack:** Bash, Xvfb, Openbox, Kitty, xdotool, ffmpeg, sha256sum, GitHub CLI, SSH, JSON Lines.

**Spec:** `docs/superpowers/specs/2026-08-23-basis-aware-workbench-agentic-validation-design.md`

## Global Constraints

- Do not run Cargo or any Cellarium build on the Raspberry Pi.
- Download `cellarium-v<TAG>-linux-aarch64.tar.gz` and verify it against the same release's `SHA256SUMS`.
- Run no GNOME, KDE, display manager, or physical display server.
- Address cleanup only by recorded PID/process-group/display identities; never use broad `pkill cellarium`, `killall`, or name-based deletion.
- Screenshots are the primary oracle. Traces and acknowledgements only correlate an action to its result.

---

### Task 1: Release acquisition and immutable run manifest

**Files:**
- Create: `scripts/agentic/fetch-release.sh`
- Create: `scripts/agentic/lib.sh`
- Test: `tests/agentic_harness.sh`

**Interfaces:**
- Produces: `fetch_release TAG OUTPUT_DIR`, which prints the absolute verified binary path and writes `release.env` containing `TAG`, `ASSET_URL`, `SHA256`, and `VERSION`.
- Produces: `agentic_require COMMAND...`, `agentic_state_dir RUN_ID`, and atomic key/value manifest helpers in `lib.sh`.

- [ ] **Step 1: Write a failing shell test for checksum rejection and manifest fields**

```bash
fake_release="$case_dir/release"
mkdir -p "$fake_release"
printf 'not-cellarium' >"$fake_release/cellarium"
printf '%064d  cellarium\n' 0 >"$fake_release/SHA256SUMS"
if scripts/agentic/fetch-release.sh --from-dir "$fake_release" "$case_dir/out"; then
  echo 'checksum mismatch was accepted' >&2
  exit 1
fi
```

- [ ] **Step 2: Run the test on tinker and verify RED**

Run: `bash tests/agentic_harness.sh release`

Expected: FAIL because `fetch-release.sh` does not exist.

- [ ] **Step 3: Implement strict release acquisition**

Implement `set -euo pipefail`, exact asset naming, `gh release download`, archive extraction into a new run directory, `sha256sum --check --strict`, `cellarium --version`, absolute-path output, and mode `0755`. `--from-dir` must exercise the same verifier without network access.

```bash
asset="cellarium-${tag}-linux-aarch64.tar.gz"
gh release download "$tag" --repo wkj2333666/Cellarium \
  --pattern "$asset" --pattern SHA256SUMS --dir "$download_dir"
(cd "$download_dir" && grep "  $asset\$" SHA256SUMS | sha256sum --check --strict -)
```

- [ ] **Step 4: Run focused tests and syntax checks**

Run: `bash -n scripts/agentic/lib.sh scripts/agentic/fetch-release.sh tests/agentic_harness.sh && bash tests/agentic_harness.sh release`

Expected: PASS; no Cargo invocation appears in the trace.

- [ ] **Step 5: Commit**

```bash
git add scripts/agentic/lib.sh scripts/agentic/fetch-release.sh tests/agentic_harness.sh
git commit -m "test: verify released agentic client artifacts"
```

### Task 2: Isolated X11 and process lifecycle

**Files:**
- Create: `scripts/agentic/session.sh`
- Modify: `tests/agentic_harness.sh`

**Interfaces:**
- Consumes: verified binary path and manifest helpers from Task 1.
- Produces: `session.sh start RUN_ID COLS ROWS -- COMMAND ARG...`, `session.sh status RUN_ID`, and `session.sh stop RUN_ID`. The quoted command vector supports both released-client C/S and `kitten ssh` direct-mode journeys without evaluation through a shell string.
- Produces manifest keys: `DISPLAY`, `XAUTHORITY`, `PROCESS_GROUP`, `KITTY_WINDOW_ID`, `CLIENT_PID`, `SCREEN_WIDTH`, `SCREEN_HEIGHT`, `COLUMNS`, and `ROWS`.

- [ ] **Step 1: Add a failing lifecycle test**

```bash
run_id="harness-$$"
scripts/agentic/session.sh start "$run_id" /usr/bin/printf unused 100 40
scripts/agentic/session.sh status "$run_id"
scripts/agentic/session.sh stop "$run_id"
! scripts/agentic/session.sh status "$run_id"
```

- [ ] **Step 2: Verify RED on tinker and the Raspberry Pi shell**

Run on tinker: `bash tests/agentic_harness.sh lifecycle-contract`

Run on Raspberry Pi: `bash tests/agentic_harness.sh lifecycle-smoke`

Expected: contract FAIL before implementation; no Rust build is run locally.

- [ ] **Step 3: Implement process-group-owned startup**

Create private mode-0700 XDG runtime/cache/config directories, choose an unused display, start one `setsid` supervisor, then start Xvfb, Openbox, and Kitty inside it. Configure a fixed screen, DPI, font, columns, and rows. Wait for the named Kitty window using `xdotool search --sync --onlyvisible`; do not sleep-and-assume.

```bash
exec setsid sh -c '
  trap "kill 0" EXIT INT TERM
  Xvfb "$DISPLAY" -screen 0 "${screen_w}x${screen_h}x24" -nolisten tcp &
  openbox &
  kitty --config "$kitty_config" --title "$window_title" "$@" &
  wait
'
```

- [ ] **Step 4: Implement identity-safe stop and leak audit**

Validate `/proc/$pid/stat` start time and the recorded process group before signaling `TERM`, wait with a deadline, escalate only that group to `KILL`, then verify the X socket, private runtime directory, client PID, and recorded tinker test-session PID are gone.

- [ ] **Step 5: Run lifecycle tests twice consecutively**

Run on Raspberry Pi: `bash tests/agentic_harness.sh lifecycle-smoke && bash tests/agentic_harness.sh lifecycle-smoke`

Expected: PASS; the second run proves the first left no conflicting display or process.

- [ ] **Step 6: Commit**

```bash
git add scripts/agentic/session.sh tests/agentic_harness.sh
git commit -m "test: isolate headless Kitty agentic sessions"
```

### Task 3: Visual action and capture primitives

**Files:**
- Create: `scripts/agentic/action.sh`
- Create: `scripts/agentic/capture.sh`
- Modify: `tests/agentic_harness.sh`

**Interfaces:**
- Produces: `action.sh RUN_ID key CHORD`, `text UTF8`, `click X Y BUTTON`, `double-click X Y BUTTON`, `drag X1 Y1 X2 Y2 BUTTON DURATION_MS`, `wheel X Y up|down COUNT`, and `resize WIDTH HEIGHT`.
- Produces: `capture.sh RUN_ID LABEL`, which writes a timestamped PNG and prints its absolute path.

- [ ] **Step 1: Add failing argument, bounds, and screenshot tests**

Assert that invalid buttons, negative coordinates, clicks outside the recorded Kitty window, and zero-size captures fail without sending input. Assert a valid capture is a nonempty PNG with the configured dimensions.

- [ ] **Step 2: Run RED**

Run on Raspberry Pi: `bash tests/agentic_harness.sh actions`

Expected: FAIL because the commands do not exist.

- [ ] **Step 3: Implement actions with current-window discovery**

Resolve the live window rectangle immediately before every action. Convert screenshot coordinates to root-window coordinates once, clamp only after rejecting out-of-window input, and use explicit X11 mouse buttons (`1`, `2`, `3`, wheel `4/5`). Implement drag as Down → timed mousemove → Up so middle-button panning is exercised.

- [ ] **Step 4: Implement framebuffer capture**

Use one-frame `ffmpeg -f x11grab` capture against the recorded `DISPLAY`; write to a temporary filename and rename only after `file` confirms a PNG. Do not crop away footer, borders, or the pointer.

- [ ] **Step 5: Prove visible keyboard and pointer effects**

Start Kitty with a held shell, capture A, inject `Ctrl+Shift+T`, capture B, click the new tab, capture C, and visually inspect A/B/C. The test script asserts only lifecycle and image validity; the executing Agent records the visual judgment.

- [ ] **Step 6: Commit**

```bash
git add scripts/agentic/action.sh scripts/agentic/capture.sh tests/agentic_harness.sh
git commit -m "test: drive and capture real Kitty windows"
```

### Task 4: Evidence ledger and current-release failing baseline

**Files:**
- Create: `scripts/agentic/evidence.sh`
- Create: `docs/agentic-testing.md`
- Create: `tests/agentic/baseline-journey.md`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `evidence.sh RUN_ID begin|action|observation|defect|finish ...`, appending JSON Lines without interpreting screenshots.
- Produces: `target/agentic/RUN_ID/{manifest.env,evidence.jsonl,frames/,logs/}` and a human-readable `report.md`.

- [ ] **Step 1: Write a failing ledger test**

Append one action with before/after image paths and assert JSON parsing succeeds, timestamps are monotonic, both images exist, and `finish pass` is rejected while any defect remains open.

- [ ] **Step 2: Implement the ledger and documented Agent loop**

Document the mandatory loop: capture → visually locate → act → wait for correlated receipt/revision where applicable → capture → visually judge → record. State explicitly that coordinates, hashes, traces, and scripts cannot declare the UX successful.

- [ ] **Step 3: Run the actual released baseline journey on the Raspberry Pi**

Download the latest released ARM64 client, start the matching `.local/bin/cellarium server` on tinker, then visually attempt: discover Workbench, click every section, draw a polygon, edit a float kernel value, edit growth source, Apply, return to Simulation, and stress-switch sections. Use only screenshots and the action primitives for decisions.

Expected: the baseline report records the currently known failures rather than claiming PASS.

- [ ] **Step 4: Audit cleanup and evidence completeness**

Verify every action has before/after frames and a visual observation, every blocker has reproduction steps, the exact release identities are present, and both local and tinker session processes are gone.

- [ ] **Step 5: Commit**

```bash
git add scripts/agentic/evidence.sh docs/agentic-testing.md tests/agentic/baseline-journey.md .gitignore
git commit -m "test: retain visual agentic UX evidence"
```

### Task 5: Harness review gate

**Files:**
- Modify only files required by findings from review, with a regression case in `tests/agentic_harness.sh` for every correction.

- [ ] **Step 1: Run all non-build harness verification**

Run on tinker: `bash -n scripts/agentic/*.sh tests/agentic_harness.sh && bash tests/agentic_harness.sh contract`

Run on Raspberry Pi: `bash tests/agentic_harness.sh lifecycle-smoke && bash tests/agentic_harness.sh actions`

- [ ] **Step 2: Review the retained baseline as a user**

Open every retained PNG in chronological order. Confirm the Agent did not use memorized coordinates, that failures are visible in final pixels, and that the report does not infer success from trace text.

- [ ] **Step 3: Commit review corrections, if any, as one focused change**

```bash
git add scripts/agentic tests/agentic_harness.sh docs/agentic-testing.md
git commit -m "test: harden visual agentic harness"
```

Expected final boundary: a reproducible, cleanly terminating real-Kitty visual journey exists and the current release has an honest retained failing baseline. No product code has changed.

