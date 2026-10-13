#!/usr/bin/env bash
# Launch script for Native Desktop GUI (Linux / macOS)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$WORKSPACE_ROOT"

if [[ -f "target/release/cloud_sync_egui" ]]; then
    echo "Starting cloud_sync_egui (release)..."
    ./target/release/cloud_sync_egui "$@" &
elif [[ -f "target/debug/cloud_sync_egui" ]]; then
    echo "Starting cloud_sync_egui (debug)..."
    ./target/debug/cloud_sync_egui "$@" &
else
    echo "Prebuilt binary not found. Running with cargo..."
    cargo run --bin cloud_sync_egui -- "$@"
fi
