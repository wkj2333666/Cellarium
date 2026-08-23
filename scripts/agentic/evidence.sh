#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

usage() {
  printf 'usage: %s RUN_ID begin JOURNEY MODE RELEASE_ENV\n' "$0" >&2
  printf '       %s RUN_ID action ID KIND DESCRIPTION BEFORE AFTER\n' "$0" >&2
  printf '       %s RUN_ID observation ACTION_ID pass|fail NOTE\n' "$0" >&2
  printf '       %s RUN_ID defect ID ACTION_ID SEVERITY SUMMARY REPRODUCTION\n' "$0" >&2
  printf '       %s RUN_ID resolve ID NOTE\n' "$0" >&2
  printf '       %s RUN_ID finish pass|fail SUMMARY\n' "$0" >&2
  exit 2
}

[[ $# -ge 2 ]] || usage
run_id=$1
operation=$2
shift 2
run_dir=$(agentic_state_dir "$run_id")
manifest="$run_dir/manifest.env"
evidence="$run_dir/evidence.jsonl"
report="$run_dir/report.md"
mkdir -p -- "$run_dir"
agentic_require jq flock realpath
exec 9>"$run_dir/evidence.lock"
flock -x 9

one_line() {
  local value=$1 label=$2
  [[ $value != *$'\n'* && $value != *$'\r'* ]] || \
    agentic_die "$label must be one line"
}

identifier() {
  [[ ${1-} =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || \
    agentic_die "unsafe identifier: ${1-}"
}

next_timestamp() {
  local now last=0
  now=$(date -u +%s%N)
  if [[ -s $evidence ]]; then
    last=$(tail -n 1 "$evidence" | jq -er '.timestamp_ns')
  fi
  if (( now <= last )); then now=$((last + 1)); fi
  printf '%s\n' "$now"
}

append_json() {
  local json=$1
  printf '%s\n' "$json" >>"$evidence"
}

ensure_started() {
  [[ -s $evidence ]] || agentic_die "evidence run has not begun: $run_id"
  ! jq -e 'select(.type == "finish")' "$evidence" >/dev/null || \
    agentic_die "evidence run is already finished: $run_id"
}

case "$operation" in
  begin)
    [[ $# == 3 ]] || usage
    journey=$1; mode=$2; release_env=$3
    identifier "$journey"
    identifier "$mode"
    [[ -f $release_env ]] || agentic_die "release manifest not found: $release_env"
    [[ ! -e $evidence ]] || agentic_die "evidence already exists: $run_id"
    # shellcheck disable=SC1090
    source "$release_env"
    : "${TAG:?missing TAG}" "${ASSET_URL:?missing ASSET_URL}" \
      "${SHA256:?missing SHA256}" "${VERSION:?missing VERSION}"
    agentic_manifest_set "$manifest" RELEASE_TAG "$TAG"
    agentic_manifest_set "$manifest" RELEASE_ASSET_URL "$ASSET_URL"
    agentic_manifest_set "$manifest" RELEASE_SHA256 "$SHA256"
    agentic_manifest_set "$manifest" RELEASE_VERSION "$VERSION"
    agentic_manifest_set "$manifest" JOURNEY "$journey"
    agentic_manifest_set "$manifest" MODE "$mode"
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" \
      --arg run_id "$run_id" --arg journey "$journey" --arg mode "$mode" \
      --arg tag "$TAG" --arg asset_url "$ASSET_URL" --arg sha256 "$SHA256" \
      --arg version "$VERSION" \
      '{timestamp_ns:$timestamp_ns,type:"begin",run_id:$run_id,journey:$journey,
        mode:$mode,release:{tag:$tag,asset_url:$asset_url,sha256:$sha256,version:$version}}')"
    printf '# Agentic journey: %s\n\n- Run: `%s`\n- Mode: `%s`\n- Release: `%s`\n\n## Events\n\n' \
      "$journey" "$run_id" "$mode" "$VERSION" >"$report"
    ;;
  action)
    [[ $# == 5 ]] || usage
    ensure_started
    action_id=$1; kind=$2; description=$3; before=$4; after=$5
    identifier "$action_id"; identifier "$kind"; one_line "$description" description
    [[ -f $before && -f $after ]] || agentic_die 'action frames must both exist'
    before=$(realpath -- "$before"); after=$(realpath -- "$after")
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" --arg id "$action_id" \
      --arg kind "$kind" --arg description "$description" --arg before "$before" \
      --arg after "$after" \
      '{timestamp_ns:$timestamp_ns,type:"action",action_id:$id,kind:$kind,
        description:$description,before_image:$before,after_image:$after}')"
    printf -- '- Action `%s` (%s): %s\n  - Before: `%s`\n  - After: `%s`\n' \
      "$action_id" "$kind" "$description" "$before" "$after" >>"$report"
    ;;
  observation)
    [[ $# == 3 ]] || usage
    ensure_started
    action_id=$1; verdict=$2; note=$3
    identifier "$action_id"
    [[ $verdict == pass || $verdict == fail ]] || agentic_die 'verdict must be pass or fail'
    one_line "$note" note
    jq -e --arg id "$action_id" 'select(.type == "action" and .action_id == $id)' \
      "$evidence" >/dev/null || agentic_die "unknown action: $action_id"
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" --arg id "$action_id" \
      --arg verdict "$verdict" --arg note "$note" \
      '{timestamp_ns:$timestamp_ns,type:"observation",action_id:$id,
        verdict:$verdict,note:$note}')"
    printf '  - Visual observation **%s**: %s\n' "$verdict" "$note" >>"$report"
    ;;
  defect)
    [[ $# == 5 ]] || usage
    ensure_started
    defect_id=$1; action_id=$2; severity=$3; summary=$4; reproduction=$5
    identifier "$defect_id"; identifier "$action_id"; identifier "$severity"
    one_line "$summary" summary; one_line "$reproduction" reproduction
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" --arg id "$defect_id" \
      --arg action_id "$action_id" --arg severity "$severity" --arg summary "$summary" \
      --arg reproduction "$reproduction" \
      '{timestamp_ns:$timestamp_ns,type:"defect",defect_id:$id,action_id:$action_id,
        severity:$severity,summary:$summary,reproduction:$reproduction}')"
    printf -- '- Defect `%s` **%s**: %s\n  - Reproduce: %s\n' \
      "$defect_id" "$severity" "$summary" "$reproduction" >>"$report"
    ;;
  resolve)
    [[ $# == 2 ]] || usage
    ensure_started
    defect_id=$1; note=$2
    identifier "$defect_id"; one_line "$note" note
    jq -e --arg id "$defect_id" 'select(.type == "defect" and .defect_id == $id)' \
      "$evidence" >/dev/null || agentic_die "unknown defect: $defect_id"
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" --arg id "$defect_id" \
      --arg note "$note" \
      '{timestamp_ns:$timestamp_ns,type:"resolve",defect_id:$id,note:$note}')"
    printf -- '- Resolved `%s`: %s\n' "$defect_id" "$note" >>"$report"
    ;;
  finish)
    [[ $# == 2 ]] || usage
    ensure_started
    verdict=$1; summary=$2
    [[ $verdict == pass || $verdict == fail ]] || agentic_die 'finish must be pass or fail'
    one_line "$summary" summary
    open_defects=$(jq -s \
      '[.[] | select(.type == "defect") | .defect_id] -
       [.[] | select(.type == "resolve") | .defect_id] | length' "$evidence")
    missing_observations=$(jq -s \
      '[.[] | select(.type == "action") | .action_id] as $actions |
       [.[] | select(.type == "observation") | .action_id] as $observations |
       [$actions[] | select(. as $id | ($observations | index($id) | not))] | length' \
      "$evidence")
    action_count=$(jq -s '[.[] | select(.type == "action")] | length' "$evidence")
    if [[ $verdict == pass ]] && \
       (( open_defects > 0 || missing_observations > 0 || action_count == 0 )); then
      agentic_die "cannot pass: $open_defects open defects, $missing_observations actions without observations, $action_count total actions"
    fi
    timestamp=$(next_timestamp)
    append_json "$(jq -cn --argjson timestamp_ns "$timestamp" --arg verdict "$verdict" \
      --arg summary "$summary" --argjson open_defects "$open_defects" \
      --argjson missing_observations "$missing_observations" \
      '{timestamp_ns:$timestamp_ns,type:"finish",verdict:$verdict,summary:$summary,
        open_defects:$open_defects,missing_observations:$missing_observations}')"
    printf '\n## Result\n\n**%s** — %s\n' "$verdict" "$summary" >>"$report"
    ;;
  *) usage ;;
esac
