#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

usage() {
  printf 'usage: %s run RUN_ID COLS ROWS -- COMMAND [ARG...]\n' "$0" >&2
  printf '       %s status RUN_ID\n' "$0" >&2
  printf '       %s client-status RUN_ID\n' "$0" >&2
  exit 2
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

session_process_matches() {
  local manifest=$1 prefix=$2 pid start
  pid=$(agentic_manifest_get "$manifest" "${prefix}_PID") || return 1
  start=$(agentic_manifest_get "$manifest" "${prefix}_START_TIME") || return 1
  agentic_process_matches "$pid" "$start"
}

status_session() {
  [[ $# == 1 ]] || usage
  local run_dir manifest
  run_dir=$(agentic_state_dir "$1")
  manifest="$run_dir/manifest.env"
  [[ -f $manifest ]] || return 1
  [[ $(agentic_manifest_get "$manifest" STATUS) == running ]] || return 1
  session_process_matches "$manifest" XVFB
  session_process_matches "$manifest" KITTY
}

client_status_session() {
  [[ $# == 1 ]] || usage
  status_session "$1"
  local run_dir manifest
  run_dir=$(agentic_state_dir "$1")
  manifest="$run_dir/manifest.env"
  session_process_matches "$manifest" CLIENT
}

run_session() {
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

  agentic_require Xvfb openbox kitty xdotool xrefresh ffmpeg file realpath
  local run_dir state_root manifest display display_number claim title
  local screen_w screen_h socket deadline window_id x_socket status
  local xvfb_pid= openbox_pid= kitty_pid= client_pid=
  run_dir=$(agentic_state_dir "$run_id")
  state_root=$(dirname -- "$run_dir")
  manifest="$run_dir/manifest.env"
  [[ ! -e $run_dir ]] || agentic_die "run already exists: $run_id"
  mkdir -p -- "$state_root" "$run_dir"/{runtime,cache,config,logs,home,frames}
  chmod 0700 "$run_dir" "$run_dir"/{runtime,cache,config,home}
  : >"$run_dir/Xauthority"
  chmod 0600 "$run_dir/Xauthority"

  display=$(choose_display "$state_root" "$run_id")
  display_number=${display#:}
  claim="$state_root/.display-$display_number.claim"
  title="cellarium-agentic-$run_id"
  screen_w=$((columns * 12 + 32))
  screen_h=$((rows * 24 + 64))
  socket="/tmp/cellarium-agentic-${display_number}.sock"

  cleanup() {
    trap - EXIT INT TERM HUP
    local pid
    agentic_manifest_set "$manifest" STATUS stopping 2>/dev/null || true
    for pid in "${client_pid-}" "${kitty_pid-}" "${openbox_pid-}" "${xvfb_pid-}"; do
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
    done
    wait 2>/dev/null || true
    rm -f -- "$socket"
    rm -rf -- "$run_dir/runtime" "$run_dir/cache" "$run_dir/config" "$run_dir/home"
    rmdir -- "$claim" 2>/dev/null || true
    agentic_manifest_set "$manifest" STATUS stopped 2>/dev/null || true
  }
  trap cleanup EXIT INT TERM HUP

  printf '%s\n' \
    'font_family monospace' \
    'font_size 14.0' \
    'allow_remote_control yes' \
    "listen_on unix:$socket" \
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
  agentic_manifest_set "$manifest" KITTY_LISTEN_ON "unix:$socket"
  agentic_manifest_set "$manifest" DISPLAY_CLAIM "$claim"
  agentic_manifest_set "$manifest" STATUS starting

  export DISPLAY=$display
  export XAUTHORITY="$run_dir/Xauthority"
  export XDG_RUNTIME_DIR="$run_dir/runtime"
  export XDG_CACHE_HOME="$run_dir/cache"
  export XDG_CONFIG_HOME="$run_dir/config"
  export HOME="$run_dir/home"
  unset NO_COLOR SSH_CONNECTION SSH_TTY

  Xvfb "$display" -screen 0 "${screen_w}x${screen_h}x24" -nolisten tcp -ac \
    >"$run_dir/logs/xvfb.log" 2>&1 &
  xvfb_pid=$!
  agentic_manifest_set "$manifest" XVFB_PID "$xvfb_pid"
  agentic_manifest_set "$manifest" XVFB_START_TIME "$(agentic_proc_start_time "$xvfb_pid")"
  x_socket="/tmp/.X11-unix/X$display_number"
  deadline=$((SECONDS + 15))
  until [[ -S $x_socket ]]; do
    kill -0 "$xvfb_pid" 2>/dev/null || agentic_die 'Xvfb exited during startup'
    (( SECONDS < deadline )) || agentic_die 'Xvfb startup timed out'
    sleep 0.05
  done

  openbox >"$run_dir/logs/openbox.log" 2>&1 &
  openbox_pid=$!
  agentic_manifest_set "$manifest" OPENBOX_PID "$openbox_pid"
  agentic_manifest_set "$manifest" OPENBOX_START_TIME "$(agentic_proc_start_time "$openbox_pid")"

  env -i \
    PATH="$PATH" HOME="$HOME" USER="${USER:-wkj}" LOGNAME="${LOGNAME:-wkj}" \
    LANG="${LANG:-C.UTF-8}" LC_ALL="${LC_ALL:-C.UTF-8}" \
    DISPLAY="$DISPLAY" XAUTHORITY="$XAUTHORITY" \
    XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" XDG_CACHE_HOME="$XDG_CACHE_HOME" \
    XDG_CONFIG_HOME="$XDG_CONFIG_HOME" \
    kitty --config "$run_dir/kitty.conf" --hold --start-as fullscreen \
    --title "$title" --class "$title" \
    /usr/bin/sh -c 'printf "%s\n" "$$" >"$1"; shift; exec "$@"' \
    sh "$run_dir/client.pid" "$@" >"$run_dir/logs/kitty.log" 2>&1 &
  kitty_pid=$!
  agentic_manifest_set "$manifest" KITTY_PID "$kitty_pid"
  agentic_manifest_set "$manifest" KITTY_START_TIME "$(agentic_proc_start_time "$kitty_pid")"

  deadline=$((SECONDS + 30))
  window_id=
  until [[ -n $window_id && -s $run_dir/client.pid ]]; do
    kill -0 "$kitty_pid" 2>/dev/null || agentic_die 'Kitty exited during startup'
    window_id=$(xdotool search --onlyvisible --name "^${title}$" 2>/dev/null | head -n 1 || true)
    (( SECONDS < deadline )) || agentic_die 'Kitty window did not become visible'
    sleep 0.05
  done
  client_pid=$(<"$run_dir/client.pid")
  agentic_manifest_set "$manifest" CLIENT_PID "$client_pid"
  agentic_manifest_set "$manifest" CLIENT_START_TIME "$(agentic_proc_start_time "$client_pid")"
  agentic_manifest_set "$manifest" KITTY_WINDOW_ID "$window_id"
  agentic_manifest_set "$manifest" STATUS running
  xrefresh -solid black
  printf 'AGENTIC_READY run=%s display=%s window=%s client=%s\n' \
    "$run_id" "$display" "$window_id" "$client_pid"

  set +e
  wait "$kitty_pid"
  status=$?
  set -e
  return "$status"
}

case "${1-}" in
  run) shift; run_session "$@" ;;
  status) shift; status_session "$@" ;;
  client-status) shift; client_status_session "$@" ;;
  *) usage ;;
esac
