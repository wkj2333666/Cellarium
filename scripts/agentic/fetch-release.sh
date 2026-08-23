#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=lib.sh
source "$script_dir/lib.sh"

usage() {
  printf 'usage: %s [--from-dir RELEASE_DIR] TAG OUTPUT_DIR\n' "$0" >&2
  exit 2
}

source_dir=
if [[ ${1-} == --from-dir ]]; then
  [[ $# == 4 ]] || usage
  source_dir=$(realpath -- "$2")
  shift 2
else
  [[ $# == 2 ]] || usage
fi

tag=$1
output_dir=$(realpath -m -- "$2")
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-.][A-Za-z0-9.]+)?$ ]] || \
  agentic_die "invalid release tag: $tag"
asset="cellarium-${tag}-linux-aarch64.tar.gz"
parent=$(dirname -- "$output_dir")
mkdir -p -- "$parent"
[[ ! -e $output_dir ]] || agentic_die "output already exists: $output_dir"

agentic_require sha256sum tar realpath mktemp

work_dir=$(mktemp -d "$parent/.agentic-release.XXXXXX")
cleanup() {
  rm -rf -- "$work_dir"
}
trap cleanup EXIT INT TERM
download_dir="$work_dir/download"
stage_dir="$work_dir/stage"
mkdir -p -- "$download_dir" "$stage_dir"

if [[ -n $source_dir ]]; then
  cp -- "$source_dir/$asset" "$download_dir/$asset"
  cp -- "$source_dir/SHA256SUMS" "$download_dir/SHA256SUMS"
  asset_url="$source_dir/$asset"
elif [[ -z ${CELLARIUM_RELEASE_BASE_URL:-} ]] && command -v gh >/dev/null 2>&1 && \
     gh auth status --hostname github.com >/dev/null 2>&1; then
  gh release download "$tag" --repo wkj2333666/Cellarium \
    --pattern "$asset" --pattern SHA256SUMS --dir "$download_dir"
  asset_url="https://github.com/wkj2333666/Cellarium/releases/download/$tag/$asset"
else
  agentic_require curl
  base_url=${CELLARIUM_RELEASE_BASE_URL:-https://github.com/wkj2333666/Cellarium/releases/download}
  base_url=${base_url%/}
  asset_url="$base_url/$tag/$asset"
  curl --fail --location --silent --show-error \
    --output "$download_dir/$asset" "$asset_url"
  curl --fail --location --silent --show-error \
    --output "$download_dir/SHA256SUMS" "$base_url/$tag/SHA256SUMS"
fi

checksum_line=$(awk -v asset="$asset" '$2 == asset { print }' \
  "$download_dir/SHA256SUMS")
[[ $(printf '%s\n' "$checksum_line" | grep -c .) == 1 ]] || \
  agentic_die "release checksum entry is missing or ambiguous: $asset"
printf '%s\n' "$checksum_line" >"$download_dir/ASSET.SHA256"
(cd "$download_dir" && sha256sum --check --strict ASSET.SHA256 >&2)
sha256=${checksum_line%% *}

tar -xzf "$download_dir/$asset" -C "$stage_dir" -- cellarium
[[ -f $stage_dir/cellarium && ! -L $stage_dir/cellarium ]] || \
  agentic_die 'archive does not contain a regular cellarium executable'
chmod 0755 "$stage_dir/cellarium"
version=$("$stage_dir/cellarium" --version)
[[ -n $version ]] || agentic_die 'cellarium --version returned an empty version'

{
  printf 'TAG=%q\n' "$tag"
  printf 'ASSET_URL=%q\n' "$asset_url"
  printf 'SHA256=%q\n' "$sha256"
  printf 'VERSION=%q\n' "$version"
} >"$stage_dir/release.env"
chmod 0600 "$stage_dir/release.env"
mv -- "$stage_dir" "$output_dir"
printf '%s/cellarium\n' "$output_dir"
