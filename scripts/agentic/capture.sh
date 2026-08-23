#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

[[ $# == 2 ]] || {
  printf 'usage: %s RUN_ID LABEL\n' "$0" >&2
  exit 2
}
run_id=$1
label=$2
[[ $label =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || agentic_die "unsafe frame label: $label"
"$script_dir/session.sh" status "$run_id" || agentic_die "session is not running: $run_id"

run_dir=$(agentic_state_dir "$run_id")
manifest="$run_dir/manifest.env"
display=$(agentic_manifest_get "$manifest" DISPLAY)
xauthority=$(agentic_manifest_get "$manifest" XAUTHORITY)
width=$(agentic_manifest_get "$manifest" SCREEN_WIDTH)
height=$(agentic_manifest_get "$manifest" SCREEN_HEIGHT)
[[ $width =~ ^[1-9][0-9]*$ && $height =~ ^[1-9][0-9]*$ ]] || \
  agentic_die 'recorded screen dimensions are invalid'

frames_dir="$run_dir/frames"
mkdir -p -- "$frames_dir"
stamp=$(date -u +%Y%m%dT%H%M%S.%N)
final="$frames_dir/${stamp}-${label}.png"
temporary=$(mktemp "$frames_dir/.capture.XXXXXX.png")
cleanup() { rm -f -- "$temporary"; }
trap cleanup EXIT INT TERM

DISPLAY=$display XAUTHORITY=$xauthority ffmpeg -nostdin -hide_banner -loglevel error \
  -f x11grab -draw_mouse 1 -video_size "${width}x${height}" -i "${display}.0+0,0" \
  -frames:v 1 -f image2 -vcodec png -y "$temporary"
[[ -s $temporary && $(file --brief --mime-type "$temporary") == image/png ]] || \
  agentic_die 'framebuffer capture did not produce a valid PNG'
mv -- "$temporary" "$final"
trap - EXIT INT TERM
printf '%s\n' "$final"
