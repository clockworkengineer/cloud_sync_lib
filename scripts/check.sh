#!/usr/bin/env bash
# Local quality check script for Linux / macOS
set -euo pipefail

echo "===> 1. Checking Code Formatting (cargo fmt)..."
cargo fmt --all -- --check

echo ""
echo "===> 2. Checking Workspace Compilation (all targets & features)..."
cargo check --workspace --all-targets --all-features

echo ""
echo "===> 3. Running Clippy Lints (with -D warnings)..."
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo ""
echo "===> 4. Running Workspace Tests..."
cargo test --workspace --all-targets

echo ""
echo "All checks passed successfully! Ready for release."
