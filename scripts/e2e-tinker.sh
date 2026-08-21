#!/usr/bin/env bash
set -euo pipefail

e2e_host="${1:-tinker}"
report_dir="${CELLARIUM_E2E_REPORT_DIR:-target/e2e}"
mkdir -p "$report_dir"

export CELLARIUM_E2E_HOST="$e2e_host"
export CELLARIUM_E2E_REPORT="$report_dir/protocol.json"
export CELLARIUM_E2E_TERMINAL_REPORT="$report_dir/terminal.json"

# The PTY probe must exercise the same optimized client users download. The
# client mirrors tinker snapshots and rasterizes them; simulation stays on the
# remote CUDA server.
cargo build --locked --release --no-default-features
export CELLARIUM_E2E_CLIENT="$(pwd)/target/release/cellarium"

if [[ -z "${CELLARIUM_E2E_SSH_CONFIG:-}" && -f "${HOME}/.ssh/config" ]]; then
    export CELLARIUM_E2E_SSH_CONFIG="${HOME}/.ssh/config"
fi

cargo test --locked --no-default-features --test remote_e2e -- \
    --ignored --nocapture --test-threads=1

echo "protocol report: $CELLARIUM_E2E_REPORT"
echo "terminal report: $CELLARIUM_E2E_TERMINAL_REPORT"
