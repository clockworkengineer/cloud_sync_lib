#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

echo "=== Cloud Sync Android APK Builder ==="

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME/ndk/27.0.12077973}"

echo "[1/4] Compiling Rust library for Android aarch64..."
cargo build --package cloud_sync_tauri --target aarch64-linux-android --lib

echo "[2/4] Copying native shared library to Gradle jniLibs..."
mkdir -p crates/cloud_sync_tauri/gen/android/app/src/main/jniLibs/arm64-v8a
cp -f target/aarch64-linux-android/debug/libcloud_sync_tauri_lib.so crates/cloud_sync_tauri/gen/android/app/src/main/jniLibs/arm64-v8a/libcloud_sync_tauri_lib.so

echo "[3/4] Syncing embedded web assets..."
mkdir -p crates/cloud_sync_tauri/gen/android/app/src/main/assets
cp -rf crates/cloud_sync_tauri/dist/* crates/cloud_sync_tauri/gen/android/app/src/main/assets/

echo "[4/4] Assembling Android APK package..."
cd crates/cloud_sync_tauri/gen/android
./gradlew assembleArm64Debug

cd ../../../..
echo ""
echo "=== BUILD COMPLETE ==="
echo "APK generated at: $(pwd)/crates/cloud_sync_tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk"
