@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0.."

echo === Cloud Sync Android Deployment Tool ===

rem Locate adb
set "ADB_EXE=adb"
where adb >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    if exist "%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe" (
        set "ADB_EXE=%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe"
    ) else if exist "%ANDROID_HOME%\platform-tools\adb.exe" (
        set "ADB_EXE=%ANDROID_HOME%\platform-tools\adb.exe"
    ) else (
        echo Error: adb executable not found.
        echo Please ensure Android SDK platform-tools is installed or added to PATH.
        exit /b 1
    )
)

echo Using ADB: "%ADB_EXE%"
echo.
echo Scanning for connected Android devices or emulators...
"%ADB_EXE%" devices -l

set "DEVICE_ID="
set "DEVICE_STATUS="

for /f "tokens=1,2" %%A in ('""%ADB_EXE%" devices"') do (
    if not "%%A"=="List" if not "%%A"=="" (
        set "DEVICE_ID=%%A"
        set "DEVICE_STATUS=%%B"
    )
)

if "%DEVICE_ID%"=="" (
    echo.
    echo [INFO] No Android device or emulator currently detected.
    echo.
    echo To connect a physical device:
    echo  1. Connect your Android phone to this PC via USB.
    echo  2. On phone: Go to Settings -^> Developer Options -^> Enable 'USB Debugging'.
    echo  3. Tap 'Allow USB debugging' when prompted on your phone screen.
    echo.
    echo To connect wirelessly:
    echo  Run: "%ADB_EXE%" connect ^<phone-ip^>:^<port^>
    echo.
    echo To use an emulator:
    echo  Start an Android Virtual Device from Android Studio Device Manager.
    exit /b 1
)

if "%DEVICE_STATUS%"=="unauthorized" (
    echo.
    echo [INFO] Device %DEVICE_ID% is unauthorized.
    echo Please unlock your Android device and tap 'Allow' on the USB debugging authorization dialog.
    exit /b 1
)

set "APK_PATH=%CD%\crates\cloud_sync_tauri\gen\android\app\build\outputs\apk\arm64\debug\app-arm64-debug.apk"
if not exist "%APK_PATH%" (
    echo.
    echo [!] APK not found at: %APK_PATH%
    echo Building APK now...
    call "%~dp0build_android.cmd"
    if %ERRORLEVEL% NEQ 0 exit /b %ERRORLEVEL%
)

echo.
echo [1/3] Installing APK to %DEVICE_ID%...
"%ADB_EXE%" -s %DEVICE_ID% install -r "%APK_PATH%"
if %ERRORLEVEL% NEQ 0 (
    echo [!] Failed to install APK.
    exit /b %ERRORLEVEL%
)

echo.
echo [2/3] Launching Cloud Sync on device...
"%ADB_EXE%" -s %DEVICE_ID% shell am start -n com.cloudsync.app/.MainActivity

echo.
echo [3/3] Streaming live application logs [Press Ctrl+C to stop]...
"%ADB_EXE%" -s %DEVICE_ID% logcat -c
"%ADB_EXE%" -s %DEVICE_ID% logcat -s "CloudSync:*" "RustStdoutStderr:*" "Tauri:*" "AndroidRuntime:E"
