#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

test_lib() (
  local case_dir state manifest
  case_dir=$(mktemp -d)
  trap 'rm -rf -- "$case_dir"' EXIT
  # shellcheck source=../scripts/agentic/lib.sh
  source "$repo_root/scripts/agentic/lib.sh"

  agentic_require sh sha256sum
  if agentic_require definitely-not-a-cellarium-command; then
    fail 'missing dependency was accepted'
  fi

  AGENTIC_TARGET_DIR="$case_dir/evidence"
  state=$(agentic_state_dir 'run-17')
  test "$state" = "$case_dir/evidence/run-17" || fail "unexpected state path: $state"
  if agentic_state_dir '../escape'; then
    fail 'unsafe run id was accepted'
  fi

  mkdir -p "$state"
  manifest="$state/manifest.env"
  agentic_manifest_set "$manifest" DISPLAY ':97'
  agentic_manifest_set "$manifest" CLIENT_PID '1234'
  agentic_manifest_set "$manifest" DISPLAY ':98'
  test "$(agentic_manifest_get "$manifest" DISPLAY)" = ':98' || \
    fail 'manifest update was not atomic/latest'
  test "$(agentic_manifest_get "$manifest" CLIENT_PID)" = '1234' || \
    fail 'manifest update lost another key'
  if agentic_manifest_set "$manifest" 'BAD-KEY' value; then
    fail 'unsafe manifest key was accepted'
  fi
  if agentic_manifest_set "$manifest" GOOD $'line1\nline2'; then
    fail 'multiline manifest value was accepted'
  fi
)

test_release() (
  local case_dir release_dir out_dir tag asset
  case_dir=$(mktemp -d)
  trap 'rm -rf -- "$case_dir"' EXIT
  release_dir="$case_dir/release"
  out_dir="$case_dir/out"
  tag=v9.8.7
  asset="cellarium-${tag}-linux-aarch64.tar.gz"
  mkdir -p "$release_dir/payload"
  printf '#!/usr/bin/env sh\nprintf "cellarium 9.8.7\\n"\n' \
    >"$release_dir/payload/cellarium"
  chmod 0755 "$release_dir/payload/cellarium"
  tar -C "$release_dir/payload" -czf "$release_dir/$asset" cellarium
  printf '%064d  %s\n' 0 "$asset" >"$release_dir/SHA256SUMS"

  if "$repo_root/scripts/agentic/fetch-release.sh" \
      --from-dir "$release_dir" "$tag" "$out_dir"; then
    fail 'checksum mismatch was accepted'
  fi

  (cd "$release_dir" && sha256sum "$asset" >SHA256SUMS)
  local binary
  binary=$("$repo_root/scripts/agentic/fetch-release.sh" \
    --from-dir "$release_dir" "$tag" "$out_dir")
  test "$binary" = "$out_dir/cellarium" || fail "unexpected binary path: $binary"
  test -x "$binary" || fail 'verified binary is not executable'

  # These literals are independently derived from the fixture above. A missing
  # field or a manifest written before verification must fail this test.
  # shellcheck disable=SC1090
  source "$out_dir/release.env"
  test "$TAG" = v9.8.7 || fail "unexpected TAG: ${TAG-}"
  test "$ASSET_URL" = "$release_dir/$asset" || \
    fail "unexpected ASSET_URL: ${ASSET_URL-}"
  test "$VERSION" = 'cellarium 9.8.7' || fail "unexpected VERSION: ${VERSION-}"
  test "$SHA256" = "$(sha256sum "$release_dir/$asset" | awk '{print $1}')" || \
    fail "unexpected SHA256: ${SHA256-}"
)

test_lifecycle_contract() (
  local case_dir run_id
  case_dir=$(mktemp -d)
  trap 'rm -rf -- "$case_dir"' EXIT
  AGENTIC_TARGET_DIR="$case_dir/state"
  export AGENTIC_TARGET_DIR

  test -x "$repo_root/scripts/agentic/session.sh" || \
    fail 'session runner is not executable'

  if "$repo_root/scripts/agentic/session.sh" status does-not-exist; then
    fail 'missing session reported as running'
  fi
  if "$repo_root/scripts/agentic/session.sh" start bad-columns nope 40 -- /usr/bin/true; then
    fail 'invalid terminal dimensions were accepted'
  fi
)

test_lifecycle_smoke() (
  local case_dir run_id manifest display
  case_dir=$(mktemp -d)
  trap 'rm -rf -- "$case_dir"' EXIT
  AGENTIC_TARGET_DIR="$case_dir/state"
  export AGENTIC_TARGET_DIR
  run_id="lifecycle-$$-$RANDOM"

  "$repo_root/scripts/agentic/session.sh" start "$run_id" 100 40 -- \
    /usr/bin/sh -c 'printf "agentic-ready\\n"; exec sleep 300'
  "$repo_root/scripts/agentic/session.sh" status "$run_id"
  manifest="$AGENTIC_TARGET_DIR/$run_id/manifest.env"
  # shellcheck source=../scripts/agentic/lib.sh
  source "$repo_root/scripts/agentic/lib.sh"
  display=$(agentic_manifest_get "$manifest" DISPLAY)
  test -S "/tmp/.X11-unix/X${display#:}" || fail 'recorded X socket is missing'
  test -n "$(agentic_manifest_get "$manifest" PROCESS_GROUP)" || \
    fail 'process group was not recorded'
  test -n "$(agentic_manifest_get "$manifest" KITTY_WINDOW_ID)" || \
    fail 'Kitty window id was not recorded'
  test -n "$(agentic_manifest_get "$manifest" CLIENT_PID)" || \
    fail 'client pid was not recorded'

  "$repo_root/scripts/agentic/session.sh" stop "$run_id"
  if "$repo_root/scripts/agentic/session.sh" status "$run_id"; then
    fail 'stopped session still reports as running'
  fi
  test ! -S "/tmp/.X11-unix/X${display#:}" || fail 'X socket leaked after stop'
)

case "${1:-contract}" in
  release) test_release ;;
  lib) test_lib ;;
  lifecycle-contract) test_lifecycle_contract ;;
  lifecycle-smoke) test_lifecycle_smoke ;;
  contract) test_lib; test_release; test_lifecycle_contract ;;
  *) fail "unknown suite: ${1-}" ;;
esac

printf 'PASS: agentic harness %s\n' "${1:-contract}"
