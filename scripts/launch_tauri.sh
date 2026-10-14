#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WORKSPACE_ROOT"

if [ -f "target/release/cloud_sync_tauri" ]; then
    ./target/release/cloud_sync_tauri &
elif [ -f "target/debug/cloud_sync_tauri" ]; then
    ./target/debug/cloud_sync_tauri &
else
    cargo run --bin cloud_sync_tauri
fi
