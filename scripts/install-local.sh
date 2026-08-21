#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo install --path "$project_dir" --root "$HOME/.local"
binary="$HOME/.local/bin/cellarium"
if [ ! -x "$binary" ]; then
    printf '%s\n' "installation did not create $binary" >&2
    exit 1
fi
printf 'installed %s\n' "$binary"
