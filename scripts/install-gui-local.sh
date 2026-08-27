#!/usr/bin/env bash
# Install a downloaded Cellarium release for the current user.
#
# It verifies the checksum before installing anything: an archive that arrived
# damaged should fail here rather than as a mysterious crash later.
set -euo pipefail

archive=${1:?usage: install-gui-local.sh <archive> [SHA256SUMS]}
sums=${2:-}
prefix=${PREFIX:-$HOME/.local}
bin_dir="$prefix/bin"
desktop_dir="$prefix/share/applications"

if [[ -n "$sums" ]]; then
  echo "verifying $(basename "$archive") against $(basename "$sums")"
  ( cd "$(dirname "$archive")" && sha256sum --ignore-missing --check "$(realpath "$sums")" )
else
  echo "no SHA256SUMS given; skipping verification" >&2
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
case "$archive" in
  *.tar.gz) tar -C "$work" -xzf "$archive" ;;
  *.zip) unzip -q -d "$work" "$archive" ;;
  *) echo "unrecognised archive: $archive" >&2; exit 1 ;;
esac

mkdir -p "$bin_dir" "$desktop_dir"
install -m 0755 "$work/cellarium" "$bin_dir/cellarium"
if [[ -f "$work/cellarium.desktop" ]]; then
  install -m 0644 "$work/cellarium.desktop" "$desktop_dir/cellarium.desktop"
fi

echo "installed $("$bin_dir/cellarium" --version)"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "note: $bin_dir is not on PATH" >&2 ;;
esac
