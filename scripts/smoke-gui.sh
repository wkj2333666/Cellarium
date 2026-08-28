#!/usr/bin/env bash
# Start the packaged GUI on a headless display, prove the window is real, and
# close it cleanly.
#
# Compiling proves the code builds. This proves the application opens a window,
# survives being looked at, and exits without being killed — which is the part
# a user actually depends on.
#
# Every wait here is a deadline rather than a fixed sleep. A first start on a
# software renderer compiles shaders, and how long that takes is a property of
# the machine, not of the application: a duration long enough for a busy shared
# runner is dead time on every other machine, and one short enough to feel quick
# fails on the slow one. Waiting for the thing itself is both faster and steadier
# than guessing how long it should take.
#
# Slowness is tolerated; failure is not. If the process dies at any point the
# wait stops immediately and prints what the application said, so a crash can
# never be mistaken for something still starting up.
set -euo pipefail

binary=${1:-target/release/cellarium}
display=${SMOKE_DISPLAY:-:99}
out_dir=${SMOKE_OUT:-target/smoke-gui}
# How long the slowest supported machine may take to show a painted window.
timeout_s=${SMOKE_TIMEOUT:-120}

if [[ ! -x "$binary" ]]; then
  echo "smoke-gui: $binary is not executable" >&2
  exit 1
fi

mkdir -p "$out_dir"
log="$out_dir/stdout.log"
# A clean data directory, so the smoke test never reads settings or an autosave
# left by a previous run and calls that a pass.
data_root=$(mktemp -d)
cleanup() {
  if [[ -n "${app_pid:-}" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  if [[ -n "${xvfb_pid:-}" ]] && kill -0 "$xvfb_pid" 2>/dev/null; then
    kill "$xvfb_pid" 2>/dev/null || true
  fi
  rm -rf "$data_root"
}
trap cleanup EXIT

# Report what the application said, then give up.
die() {
  echo "smoke-gui: $1" >&2
  if [[ -s "$log" ]]; then
    echo "smoke-gui: the application's output follows" >&2
    cat "$log" >&2
  else
    echo "smoke-gui: the application printed nothing" >&2
  fi
  exit 1
}

Xvfb "$display" -screen 0 1440x900x24 >/dev/null 2>&1 &
xvfb_pid=$!

# The display has to exist before anything is asked of it. Probed with xdotool
# because the script already depends on it: xdpyinfo would be the obvious tool
# and ships in a different package, so reaching for it would add an install
# every caller has to know about.
deadline=$((SECONDS + timeout_s))
until DISPLAY="$display" xdotool getdisplaygeometry >/dev/null 2>&1; do
  kill -0 "$xvfb_pid" 2>/dev/null || die "Xvfb exited before the display was ready"
  ((SECONDS < deadline)) || die "the display was not ready within ${timeout_s}s"
  sleep 1
done

DISPLAY="$display" XDG_DATA_HOME="$data_root" "$binary" >"$log" 2>&1 &
app_pid=$!

# Wait for a window rather than for a duration.
window=""
deadline=$((SECONDS + timeout_s))
while [[ -z "$window" ]]; do
  kill -0 "$app_pid" 2>/dev/null || die "the application exited before a window appeared"
  ((SECONDS < deadline)) || die "no Cellarium window appeared within ${timeout_s}s"
  window=$(DISPLAY="$display" xdotool search --name Cellarium 2>/dev/null | head -1 || true)
  [[ -n "$window" ]] || sleep 1
done

geometry=$(DISPLAY="$display" xdotool getwindowgeometry "$window")
echo "smoke-gui: $geometry"

# A window that is present but painting nothing is not a working application.
# The first frame can lag the window on a software renderer, so this waits for
# paint the same way it waited for the window.
deadline=$((SECONDS + timeout_s))
while true; do
  kill -0 "$app_pid" 2>/dev/null || die "the application exited before it painted"
  DISPLAY="$display" xwd -root -silent | convert xwd:- "$out_dir/window.png"
  colors=$(identify -format '%k' "$out_dir/window.png")
  if ((colors >= 8)); then
    break
  fi
  ((SECONDS < deadline)) || die "the window painted only $colors distinct colours in ${timeout_s}s"
  sleep 1
done
echo "smoke-gui: window painted $colors distinct colours"

# Ask it to close, and require that it goes on its own rather than being killed.
DISPLAY="$display" xdotool windowkill "$window" 2>/dev/null || kill "$app_pid"
deadline=$((SECONDS + timeout_s))
while kill -0 "$app_pid" 2>/dev/null; do
  ((SECONDS < deadline)) || die "the application did not exit within ${timeout_s}s of its window closing"
  sleep 1
done
unset app_pid
echo "smoke-gui: exited cleanly"
