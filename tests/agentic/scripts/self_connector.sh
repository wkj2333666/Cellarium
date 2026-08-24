#!/usr/bin/env bash
set -euo pipefail

# Test-only connector for running the real C/S viewer and server inside one
# isolated agentic X11 session. run_connect appends HOST REMOTE_BINARY server;
# the harness intentionally ignores those transport arguments and launches
# the just-built server from the worktree instead.
exec target/release/cellarium server
