#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

usage() {
  printf 'usage: %s RUN_ID key CHORD\n' "$0" >&2
  printf '       %s RUN_ID text UTF8\n' "$0" >&2
  printf '       %s RUN_ID click X Y BUTTON\n' "$0" >&2
  printf '       %s RUN_ID move X Y\n' "$0" >&2
  printf '       %s RUN_ID double-click X Y BUTTON\n' "$0" >&2
  printf '       %s RUN_ID drag X1 Y1 X2 Y2 BUTTON DURATION_MS\n' "$0" >&2
  printf '       %s RUN_ID wheel X Y up|down COUNT\n' "$0" >&2
  printf '       %s RUN_ID resize WIDTH HEIGHT\n' "$0" >&2
  exit 2
}

[[ $# -ge 2 ]] || usage
run_id=$1
action=$2
shift 2
run_dir=$(agentic_state_dir "$run_id")
manifest="$run_dir/manifest.env"
"$script_dir/session.sh" client-status "$run_id" || agentic_die "client is not running: $run_id"
display=$(agentic_manifest_get "$manifest" DISPLAY)
xauthority=$(agentic_manifest_get "$manifest" XAUTHORITY)
window_id=$(agentic_manifest_get "$manifest" KITTY_WINDOW_ID)
export DISPLAY=$display XAUTHORITY=$xauthority

read_geometry() {
  local geometry
  geometry=$(xdotool getwindowgeometry --shell "$window_id")
  window_x=$(awk -F= '$1 == "X" { print $2 }' <<<"$geometry")
  window_y=$(awk -F= '$1 == "Y" { print $2 }' <<<"$geometry")
  window_width=$(awk -F= '$1 == "WIDTH" { print $2 }' <<<"$geometry")
  window_height=$(awk -F= '$1 == "HEIGHT" { print $2 }' <<<"$geometry")
  [[ $window_x =~ ^-?[0-9]+$ && $window_y =~ ^-?[0-9]+$ && \
     $window_width =~ ^[1-9][0-9]*$ && $window_height =~ ^[1-9][0-9]*$ ]]
}

require_coordinate() {
  local value=$1 label=$2
  [[ $value =~ ^[0-9]+$ ]] || agentic_die "$label must be a non-negative integer"
}

require_point_in_window() {
  local point_x=$1 point_y=$2
  require_coordinate "$point_x" x
  require_coordinate "$point_y" y
  read_geometry
  (( point_x >= window_x && point_x < window_x + window_width &&
     point_y >= window_y && point_y < window_y + window_height )) || \
    agentic_die "point is outside the current Kitty window: $point_x,$point_y"
}

require_button() {
  [[ ${1-} =~ ^[123]$ ]] || agentic_die 'mouse button must be 1, 2, or 3'
}

activate_window() {
  local active_window
  active_window=$(xdotool getactivewindow 2>/dev/null || true)
  if [[ $active_window != "$window_id" ]]; then
    xdotool windowactivate --sync "$window_id"
  fi
}

case "$action" in
  key)
    [[ $# == 1 && $1 =~ ^[A-Za-z0-9_+:.=-]+$ ]] || usage
    key_name=$1
    [[ $key_name == enter ]] && key_name=Return
    [[ $key_name == esc ]] && key_name=Escape
    activate_window
    xdotool key --clearmodifiers "$key_name"
    ;;
  text)
    [[ $# == 1 ]] || usage
    activate_window
    xdotool type --clearmodifiers --delay 20 -- "$1"
    ;;
  click|double-click)
    [[ $# == 3 ]] || usage
    require_point_in_window "$1" "$2"
    require_button "$3"
    activate_window
    xdotool mousemove "$1" "$2"
    if [[ $action == click ]]; then
      xdotool click "$3"
    else
      xdotool click --repeat 2 --delay 120 "$3"
    fi
    ;;
  move)
    [[ $# == 2 ]] || usage
    require_point_in_window "$1" "$2"
    activate_window
    xdotool mousemove "$1" "$2"
    ;;
  drag)
    [[ $# == 6 ]] || usage
    require_point_in_window "$1" "$2"
    require_point_in_window "$3" "$4"
    require_button "$5"
    [[ $6 =~ ^[1-9][0-9]*$ ]] || agentic_die 'drag duration must be positive milliseconds'
    (( $6 <= 10000 )) || agentic_die 'drag duration exceeds 10000 ms'
    activate_window
    xdotool mousemove "$1" "$2"
    xdotool mousedown "$5"
    start_x=$1; start_y=$2; end_x=$3; end_y=$4; button=$5; duration=$6
    steps=12
    step_delay=$(awk -v duration="$duration" -v steps="$steps" \
      'BEGIN { printf "%.4f", duration / steps / 1000 }')
    for ((step = 1; step <= steps; step++)); do
      next_x=$((start_x + (end_x - start_x) * step / steps))
      next_y=$((start_y + (end_y - start_y) * step / steps))
      xdotool mousemove "$next_x" "$next_y"
      sleep "$step_delay"
    done
    xdotool mouseup "$button"
    ;;
  wheel)
    [[ $# == 4 ]] || usage
    require_point_in_window "$1" "$2"
    [[ $3 == up || $3 == down ]] || agentic_die 'wheel direction must be up or down'
    [[ $4 =~ ^[1-9][0-9]*$ ]] || agentic_die 'wheel count must be positive'
    (( $4 <= 100 )) || agentic_die 'wheel count exceeds 100'
    activate_window
    xdotool mousemove "$1" "$2"
    if [[ $3 == up ]]; then button=4; else button=5; fi
    xdotool click --repeat "$4" --delay 25 "$button"
    ;;
  resize)
    [[ $# == 2 ]] || usage
    require_coordinate "$1" width
    require_coordinate "$2" height
    screen_width=$(agentic_manifest_get "$manifest" SCREEN_WIDTH)
    screen_height=$(agentic_manifest_get "$manifest" SCREEN_HEIGHT)
    (( $1 >= 320 && $2 >= 200 && $1 <= screen_width && $2 <= screen_height )) || \
      agentic_die 'requested window size is outside the X screen'
    xdotool windowsize --sync "$window_id" "$1" "$2"
    ;;
  *) usage ;;
esac
