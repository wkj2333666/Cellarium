#!/usr/bin/env bash

agentic_die() {
  printf 'agentic: %s\n' "$*" >&2
  return 1
}

agentic_require() {
  local command
  for command in "$@"; do
    command -v "$command" >/dev/null 2>&1 || {
      agentic_die "required command not found: $command"
      return 1
    }
  done
}

agentic_state_dir() {
  local run_id=${1-} lib_dir repo_root target_dir
  [[ $run_id =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || {
    agentic_die "unsafe run id: $run_id"
    return 1
  }
  lib_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
  repo_root=$(cd "$lib_dir/../../.." && pwd -P)
  target_dir=${AGENTIC_TARGET_DIR:-$repo_root/target/agentic-tinker}
  printf '%s/%s\n' "$(realpath -m -- "$target_dir")" "$run_id"
}

agentic_manifest_set() {
  local manifest=${1-} key=${2-} value=${3-} dir tmp
  [[ $key =~ ^[A-Z][A-Z0-9_]*$ ]] || {
    agentic_die "unsafe manifest key: $key"
    return 1
  }
  [[ $value != *$'\n'* && $value != *$'\r'* ]] || {
    agentic_die 'manifest values must be one line'
    return 1
  }
  dir=$(dirname -- "$manifest")
  mkdir -p -- "$dir"
  tmp=$(mktemp "$dir/.manifest.XXXXXX")
  chmod 0600 "$tmp"
  if [[ -f $manifest ]]; then
    awk -v prefix="$key=" 'index($0, prefix) != 1 { print }' "$manifest" >"$tmp"
  fi
  printf '%s=' "$key" >>"$tmp"
  printf '%q' "$value" >>"$tmp"
  printf '\n' >>"$tmp"
  mv -f -- "$tmp" "$manifest"
}

agentic_manifest_get() {
  local manifest=${1-} key=${2-}
  [[ $key =~ ^[A-Z][A-Z0-9_]*$ ]] || {
    agentic_die "unsafe manifest key: $key"
    return 1
  }
  [[ -f $manifest ]] || {
    agentic_die "manifest not found: $manifest"
    return 1
  }
  bash -c 'set -u; source "$1"; printf "%s\n" "${!2}"' bash "$manifest" "$key"
}

agentic_proc_start_time() {
  local pid=${1-} stat
  [[ $pid =~ ^[1-9][0-9]*$ && -r /proc/$pid/stat ]] || return 1
  stat=$(<"/proc/$pid/stat")
  stat=${stat##*) }
  awk '{ print $20 }' <<<"$stat"
}

agentic_process_matches() {
  local pid=${1-} expected=${2-} actual
  actual=$(agentic_proc_start_time "$pid") || return 1
  [[ $actual == "$expected" ]]
}
