#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

usage() {
  printf 'usage: %s start RUN_ID COLS ROWS -- COMMAND [ARG...]\n' "$0" >&2
  printf '       %s status RUN_ID\n' "$0" >&2
  printf '       %s stop RUN_ID\n' "$0" >&2
  exit 2
}

proc_start_time() {
  local pid=${1-} rest
  [[ $pid =~ ^[1-9][0-9]*$ && -r /proc/$pid/stat ]] || return 1
  rest=$(<"/proc/$pid/stat")
  rest=${rest##*) }
  awk '{ print $20 }' <<<"$rest"
}

proc_group() {
  local pid=${1-}
  [[ $pid =~ ^[1-9][0-9]*$ && -d /proc/$pid ]] || return 1
  ps -o pgid= -p "$pid" | tr -d ' '
}

session_identity() {
  local manifest=$1 pid expected_start expected_group actual_start actual_group
  pid=$(agentic_manifest_get "$manifest" SUPERVISOR_PID) || return 1
  expected_start=$(agentic_manifest_get "$manifest" SUPERVISOR_START_TIME) || return 1
  expected_group=$(agentic_manifest_get "$manifest" PROCESS_GROUP) || return 1
  actual_start=$(proc_start_time "$pid") || return 1
  actual_group=$(proc_group "$pid") || return 1
  [[ $actual_start == "$expected_start" && $actual_group == "$expected_group" && \
     $pid == "$expected_group" ]]
}

choose_display() {
  local claim_root=$1 run_id=$2 number claim
  for number in $(seq 90 199); do
    [[ ! -S /tmp/.X11-unix/X$number && ! -e /tmp/.X${number}-lock ]] || continue
    claim="$claim_root/.display-$number.claim"
    if mkdir -- "$claim" 2>/dev/null; then
      printf '%s\n' "$run_id" >"$claim/run-id"
      printf ':%s\n' "$number"
      return 0
    fi
  done
  agentic_die 'no unused X11 display is available'
}

supervise() {
  local run_dir=$1 display=$2 screen_w=$3 screen_h=$4 title=$5
  shift 5
  local runtime_dir="$run_dir/runtime" cache_dir="$run_dir/cache"
  local config_dir="$run_dir/config" logs_dir="$run_dir/logs"
  local xvfb_pid= openbox_pid= kitty_pid=

  cleanup_children() {
    trap - EXIT INT TERM
    local pid
    for pid in "$kitty_pid" "$openbox_pid" "$xvfb_pid"; do
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
    done
    wait 2>/dev/null || true
  }
  trap cleanup_children EXIT INT TERM

  export DISPLAY=$display
  export XAUTHORITY="$run_dir/Xauthority"
  export XDG_RUNTIME_DIR=$runtime_dir
  export XDG_CACHE_HOME=$cache_dir
  export XDG_CONFIG_HOME=$config_dir
  export HOME="$run_dir/home"

  Xvfb "$display" -screen 0 "${screen_w}x${screen_h}x24" -nolisten tcp -ac \
    >"$logs_dir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  printf '%s\n' "$xvfb_pid" >"$run_dir/xvfb.pid"
  local socket="/tmp/.X11-unix/X${display#:}" deadline=$((SECONDS + 15))
  until [[ -S $socket ]]; do
    kill -0 "$xvfb_pid" 2>/dev/null || exit 1
    (( SECONDS < deadline )) || exit 1
    sleep 0.05
  done

  openbox >"$logs_dir/openbox.log" 2>&1 &
  openbox_pid=$!
  printf '%s\n' "$openbox_pid" >"$run_dir/openbox.pid"

  kitty --config "$run_dir/kitty.conf" --start-as fullscreen \
    --title "$title" --class "$title" \
    /usr/bin/sh -c 'printf "%s\n" "$$" >"$1"; shift; exec "$@"' \
    sh "$run_dir/client.pid" "$@" >"$logs_dir/kitty.log" 2>&1 &
  kitty_pid=$!
  printf '%s\n' "$kitty_pid" >"$run_dir/kitty.pid"
  wait "$kitty_pid"
}

start_session() {
  [[ $# -ge 5 ]] || usage
  local run_id=$1 columns=$2 rows=$3
  shift 3
  [[ $1 == -- ]] || usage
  shift
  [[ $# -ge 1 ]] || usage
  [[ $columns =~ ^[1-9][0-9]*$ && $rows =~ ^[1-9][0-9]*$ ]] || \
    agentic_die 'columns and rows must be positive integers'
  (( columns >= 40 && columns <= 300 && rows >= 12 && rows <= 120 )) || \
    agentic_die 'terminal dimensions are outside the safe range'

  agentic_require Xvfb openbox kitty xdotool xrefresh setsid ps realpath
  local run_dir manifest state_root display display_number claim title
  local screen_w screen_h supervisor_pid supervisor_start pgid window_id deadline
  run_dir=$(agentic_state_dir "$run_id")
  state_root=$(dirname -- "$run_dir")
  manifest="$run_dir/manifest.env"
  [[ ! -e $run_dir ]] || agentic_die "run already exists: $run_id"
  mkdir -p -- "$state_root" "$run_dir"/{runtime,cache,config,logs,home,frames}
  chmod 0700 "$run_dir" "$run_dir"/{runtime,cache,config,home}
  : >"$run_dir/Xauthority"
  chmod 0600 "$run_dir/Xauthority"

  display=$(choose_display "$state_root" "$run_id") || {
    rmdir -- "$run_dir"/{runtime,cache,config,logs,home,frames} "$run_dir" 2>/dev/null || true
    return 1
  }
  display_number=${display#:}
  claim="$state_root/.display-$display_number.claim"
  title="cellarium-agentic-$run_id"
  screen_w=$((columns * 12 + 32))
  screen_h=$((rows * 24 + 64))
  printf '%s\n' \
    'font_family monospace' \
    'font_size 14.0' \
    'remember_window_size no' \
    "initial_window_width ${columns}c" \
    "initial_window_height ${rows}c" \
    'window_padding_width 0' \
    'confirm_os_window_close 0' \
    'enable_audio_bell no' \
    'shell_integration disabled' \
    >"$run_dir/kitty.conf"

  agentic_manifest_set "$manifest" RUN_ID "$run_id"
  agentic_manifest_set "$manifest" DISPLAY "$display"
  agentic_manifest_set "$manifest" XAUTHORITY "$run_dir/Xauthority"
  agentic_manifest_set "$manifest" SCREEN_WIDTH "$screen_w"
  agentic_manifest_set "$manifest" SCREEN_HEIGHT "$screen_h"
  agentic_manifest_set "$manifest" COLUMNS "$columns"
  agentic_manifest_set "$manifest" ROWS "$rows"
  agentic_manifest_set "$manifest" WINDOW_TITLE "$title"
  agentic_manifest_set "$manifest" DISPLAY_CLAIM "$claim"

  setsid "$0" _supervise "$run_dir" "$display" "$screen_w" "$screen_h" \
    "$title" "$@" >"$run_dir/logs/supervisor.log" 2>&1 &
  supervisor_pid=$!
  deadline=$((SECONDS + 5))
  until supervisor_start=$(proc_start_time "$supervisor_pid") && \
        pgid=$(proc_group "$supervisor_pid") && [[ $pgid == "$supervisor_pid" ]]; do
    (( SECONDS < deadline )) || {
      kill -TERM "$supervisor_pid" 2>/dev/null || true
      agentic_die 'supervisor did not establish its process group'
      return 1
    }
    sleep 0.05
  done
  agentic_manifest_set "$manifest" SUPERVISOR_PID "$supervisor_pid"
  agentic_manifest_set "$manifest" SUPERVISOR_START_TIME "$supervisor_start"
  agentic_manifest_set "$manifest" PROCESS_GROUP "$pgid"

  deadline=$((SECONDS + 20))
  window_id=
  until [[ -n $window_id && -s $run_dir/client.pid ]]; do
    if ! kill -0 "$supervisor_pid" 2>/dev/null; then
      printf 'agentic: session startup failed; see %s\n' "$run_dir/logs" >&2
      return 1
    fi
    window_id=$(DISPLAY="$display" XAUTHORITY="$run_dir/Xauthority" \
      xdotool search --onlyvisible --name "^${title}$" 2>/dev/null | head -n 1 || true)
    (( SECONDS < deadline )) || {
      "$0" stop "$run_id" || true
      agentic_die 'Kitty window did not become visible'
      return 1
    }
    sleep 0.05
  done
  DISPLAY="$display" XAUTHORITY="$run_dir/Xauthority" xrefresh -solid black
  agentic_manifest_set "$manifest" KITTY_WINDOW_ID "$window_id"
  agentic_manifest_set "$manifest" CLIENT_PID "$(<"$run_dir/client.pid")"
  agentic_manifest_set "$manifest" XVFB_PID "$(<"$run_dir/xvfb.pid")"
  agentic_manifest_set "$manifest" STATUS running
  printf '%s\n' "$run_dir"
}

status_session() {
  [[ $# == 1 ]] || usage
  local run_dir manifest
  run_dir=$(agentic_state_dir "$1")
  manifest="$run_dir/manifest.env"
  [[ -f $manifest ]] || return 1
  session_identity "$manifest"
}

stop_session() {
  [[ $# == 1 ]] || usage
  local run_dir manifest pid pgid display claim deadline
  run_dir=$(agentic_state_dir "$1")
  manifest="$run_dir/manifest.env"
  [[ -f $manifest ]] || return 1
  if session_identity "$manifest"; then
    pid=$(agentic_manifest_get "$manifest" SUPERVISOR_PID)
    pgid=$(agentic_manifest_get "$manifest" PROCESS_GROUP)
    kill -TERM -- "-$pgid" 2>/dev/null || true
    deadline=$((SECONDS + 8))
    while [[ -d /proc/$pid ]] && (( SECONDS < deadline )); do sleep 0.05; done
    if [[ -d /proc/$pid ]] && session_identity "$manifest"; then
      kill -KILL -- "-$pgid" 2>/dev/null || true
    fi
  elif [[ -n $(agentic_manifest_get "$manifest" SUPERVISOR_PID 2>/dev/null || true) ]]; then
    agentic_die 'refusing to stop a session whose process identity changed'
    return 1
  fi

  display=$(agentic_manifest_get "$manifest" DISPLAY)
  claim=$(agentic_manifest_get "$manifest" DISPLAY_CLAIM)
  deadline=$((SECONDS + 5))
  while [[ -S /tmp/.X11-unix/X${display#:} ]] && (( SECONDS < deadline )); do sleep 0.05; done
  [[ ! -S /tmp/.X11-unix/X${display#:} ]] || \
    agentic_die "X socket remained after session stop: $display"
  rm -rf -- "$run_dir/runtime" "$run_dir/cache" "$run_dir/config" "$run_dir/home"
  rm -rf -- "$claim"
  agentic_manifest_set "$manifest" STATUS stopped
}

case "${1-}" in
  start) shift; start_session "$@" ;;
  status) shift; status_session "$@" ;;
  stop) shift; stop_session "$@" ;;
  _supervise) shift; supervise "$@" ;;
  *) usage ;;
esac
