#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

echo "=== Cloud Sync Android Deployment Tool ==="

# Locate adb
ADB_CMD="adb"
if ! command -v adb >/dev/null 2>&1; then
    if [ -n "$ANDROID_HOME" ] && [ -x "$ANDROID_HOME/platform-tools/adb" ]; then
        ADB_CMD="$ANDROID_HOME/platform-tools/adb"
    elif [ -x "$HOME/Android/Sdk/platform-tools/adb" ]; then
        ADB_CMD="$HOME/Android/Sdk/platform-tools/adb"
    elif [ -x "$HOME/Library/Android/sdk/platform-tools/adb" ]; then
        ADB_CMD="$HOME/Library/Android/sdk/platform-tools/adb"
    else
        echo "Error: adb executable not found in PATH or standard Android SDK paths."
        exit 1
    fi
fi

echo "Using ADB: $ADB_CMD"
echo ""
echo "Scanning for connected Android devices or emulators..."
"$ADB_CMD" devices -l

DEVICE_ID=$("$ADB_CMD" devices | grep -v "List of devices" | grep -v "^$" | awk 'NR==1{print $1}')
DEVICE_STATUS=$("$ADB_CMD" devices | grep -v "List of devices" | grep -v "^$" | awk 'NR==1{print $2}')

if [ -z "$DEVICE_ID" ]; then
    echo ""
    echo "[!] No Android device or emulator currently detected."
    echo ""
    echo "To connect a device:"
    echo "  1. Connect your Android phone via USB cable."
    echo "  2. On phone: Go to Settings -> Developer Options -> Enable 'USB Debugging'."
    echo "  3. Tap 'Allow USB debugging' when prompted on your phone screen."
    echo ""
    echo "To connect wirelessly:"
    echo "  Run: $ADB_CMD connect <phone-ip>:<port>"
    echo ""
    echo "To use an emulator:"
    echo "  Start an Android Virtual Device (AVD) from Android Studio."
    exit 1
fi

if [ "$DEVICE_STATUS" = "unauthorized" ]; then
    echo ""
    echo "[!] Device $DEVICE_ID is unauthorized."
    echo "Please unlock your Android device and tap 'Allow' on the USB debugging authorization dialog."
    exit 1
fi

APK_PATH="crates/cloud_sync_tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk"
if [ ! -f "$APK_PATH" ]; then
    echo ""
    echo "[!] APK not found at: $APK_PATH"
    echo "Building APK now..."
    ./scripts/build_android.sh
fi

echo ""
echo "[1/3] Installing APK to $DEVICE_ID..."
"$ADB_CMD" -s "$DEVICE_ID" install -r "$APK_PATH"

echo ""
echo "[2/3] Launching Cloud Sync on device..."
"$ADB_CMD" -s "$DEVICE_ID" shell am start -n com.cloudsync.app/.MainActivity

echo ""
echo "[3/3] Streaming live application logs (Press Ctrl+C to stop)..."
"$ADB_CMD" -s "$DEVICE_ID" logcat -c
"$ADB_CMD" -s "$DEVICE_ID" logcat -s "CloudSync:*" "RustStdoutStderr:*" "Tauri:*" "AndroidRuntime:E"
