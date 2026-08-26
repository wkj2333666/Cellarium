#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
agentic_dir="$script_dir/agentic"
# shellcheck source=agentic/lib.sh
source "$agentic_dir/lib.sh"

usage() {
  cat >&2 <<'USAGE'
usage: agentic-workbench-journey.sh start RUN_ID kitty|half-block RELEASE_DIR HOST [COLS ROWS]
       agentic-workbench-journey.sh capture RUN_ID LABEL
       agentic-workbench-journey.sh action RUN_ID key|text|click|double-click|drag|wheel|resize ARGS...
       agentic-workbench-journey.sh record RUN_ID ACTION_ID KIND DESCRIPTION BEFORE AFTER
       agentic-workbench-journey.sh observe RUN_ID ACTION_ID pass|fail NOTE
       agentic-workbench-journey.sh defect RUN_ID ID ACTION_ID SEVERITY SUMMARY REPRODUCTION
       agentic-workbench-journey.sh resolve RUN_ID ID NOTE
       agentic-workbench-journey.sh finish RUN_ID pass|fail SUMMARY
       agentic-workbench-journey.sh status RUN_ID
       agentic-workbench-journey.sh stop RUN_ID
USAGE
  exit 2
}

[[ $# -ge 2 ]] || usage
operation=$1
run_id=$2
shift 2

case "$operation" in
  start)
    [[ $# == 3 || $# == 5 ]] || usage
    mode=$1
    release_dir=$(realpath -- "$2")
    host=$3
    columns=${4:-160}
    rows=${5:-40}
    [[ $mode == kitty || $mode == half-block ]] || usage
    [[ $host =~ ^[A-Za-z0-9._-]+$ ]] || agentic_die "unsafe SSH host: $host"
    [[ -x $release_dir/cellarium && -f $release_dir/release.env ]] || +      agentic_die "verified release directory is incomplete: $release_dir"
    run_dir=$(agentic_state_dir "$run_id")
    graphics=1
    [[ $mode == half-block ]] && graphics=0
    if ! "$agentic_dir/session.sh" start "$run_id" "$columns" "$rows" -- +      env "CELLARIUM_REMOTE_GRAPHICS=$graphics" +      "XDG_DATA_HOME=$run_dir/data" +      "$release_dir/cellarium" connect "$host"; then
      exit 1
    fi
    if ! "$agentic_dir/evidence.sh" "$run_id" begin workbench "$mode" +      "$release_dir/release.env"; then
      "$agentic_dir/session.sh" stop "$run_id" || true
      exit 1
    fi
    printf '%s\n' "$run_dir"
    ;;
  capture)
    [[ $# == 1 ]] || usage
    exec "$agentic_dir/capture.sh" "$run_id" "$1"
    ;;
  action)
    [[ $# -ge 2 ]] || usage
    action=$1
    shift
    exec "$agentic_dir/action.sh" "$run_id" "$action" "$@"
    ;;
  record)
    [[ $# == 5 ]] || usage
    exec "$agentic_dir/evidence.sh" "$run_id" action "$@"
    ;;
  observe)
    [[ $# == 3 ]] || usage
    exec "$agentic_dir/evidence.sh" "$run_id" observation "$@"
    ;;
  defect)
    [[ $# == 5 ]] || usage
    exec "$agentic_dir/evidence.sh" "$run_id" defect "$@"
    ;;
  resolve)
    [[ $# == 2 ]] || usage
    exec "$agentic_dir/evidence.sh" "$run_id" resolve "$@"
    ;;
  finish)
    [[ $# == 2 ]] || usage
    "$agentic_dir/evidence.sh" "$run_id" finish "$@"
    "$agentic_dir/session.sh" stop "$run_id"
    ;;
  status)
    [[ $# == 0 ]] || usage
    exec "$agentic_dir/session.sh" status "$run_id"
    ;;
  stop)
    [[ $# == 0 ]] || usage
    exec "$agentic_dir/session.sh" stop "$run_id"
    ;;
  *) usage ;;
esac
