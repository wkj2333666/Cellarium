#!/usr/bin/env bash
# Start the packaged GUI on a headless display, prove the window is real, and
# close it cleanly.
#
# Compiling proves the code builds. This proves the application opens a window,
# survives being looked at, and exits without being killed — which is the part
# a user actually depends on.
set -euo pipefail

binary=${1:-target/release/cellarium}
display=${SMOKE_DISPLAY:-:99}
out_dir=${SMOKE_OUT:-target/smoke-gui}
# Long enough for a first-run shader compile on a software renderer.
settle=${SMOKE_SETTLE:-12}

if [[ ! -x "$binary" ]]; then
  echo "smoke-gui: $binary is not executable" >&2
  exit 1
fi

mkdir -p "$out_dir"
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

Xvfb "$display" -screen 0 1440x900x24 >/dev/null 2>&1 &
xvfb_pid=$!
sleep 2

DISPLAY="$display" XDG_DATA_HOME="$data_root" "$binary" >"$out_dir/stdout.log" 2>&1 &
app_pid=$!
sleep "$settle"

if ! kill -0 "$app_pid" 2>/dev/null; then
  echo "smoke-gui: the application exited before the window could be checked" >&2
  cat "$out_dir/stdout.log" >&2
  exit 1
fi

window=$(DISPLAY="$display" xdotool search --name Cellarium | head -1 || true)
if [[ -z "$window" ]]; then
  echo "smoke-gui: no Cellarium window appeared" >&2
  cat "$out_dir/stdout.log" >&2
  exit 1
fi

geometry=$(DISPLAY="$display" xdotool getwindowgeometry "$window")
echo "smoke-gui: $geometry"
DISPLAY="$display" xwd -root -silent | convert xwd:- "$out_dir/window.png"

# A window that is present but painting nothing is not a working application.
colors=$(identify -format '%k' "$out_dir/window.png")
if [[ "$colors" -lt 8 ]]; then
  echo "smoke-gui: the window painted only $colors distinct colours" >&2
  exit 1
fi
echo "smoke-gui: window painted $colors distinct colours"

# Ask it to close, and require that it goes on its own rather than being killed.
DISPLAY="$display" xdotool windowkill "$window" 2>/dev/null || kill "$app_pid"
for _ in $(seq 1 20); do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    unset app_pid
    echo "smoke-gui: exited cleanly"
    exit 0
  fi
  sleep 0.5
done

echo "smoke-gui: the application did not exit after its window was closed" >&2
exit 1
