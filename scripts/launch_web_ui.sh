#!/usr/bin/env bash
# Launch script for Axum Web Dashboard (Linux / macOS)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$WORKSPACE_ROOT"

# Start the Web UI server in the background
if [[ -f "target/release/cloud_sync_ui" ]]; then
    echo "Starting cloud_sync_ui (release)..."
    ./target/release/cloud_sync_ui "$@" &
elif [[ -f "target/debug/cloud_sync_ui" ]]; then
    echo "Starting cloud_sync_ui (debug)..."
    ./target/debug/cloud_sync_ui "$@" &
else
    echo "Prebuilt binary not found. Running with cargo..."
    cargo run --bin cloud_sync_ui -- "$@" &
fi

UI_URL="http://127.0.0.1:8082"
echo "Web UI launched at ${UI_URL}"

# Allow background process a brief moment to bind listener before opening browser
sleep 1

# Open in default browser depending on operating system
if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "${UI_URL}" >/dev/null 2>&1 &
elif command -v open >/dev/null 2>&1; then
    open "${UI_URL}"
fi
