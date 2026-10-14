@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0.."

echo === Cloud Sync Android APK Builder ===

for /d %%i in ("%USERPROFILE%\.jdk\jdk-17*") do set "JAVA_HOME=%%i"
if "%JAVA_HOME%"=="" (
    echo Error: OpenJDK 17 not found.
    exit /b 1
)

set "PATH=%JAVA_HOME%\bin;%PATH%"
set "ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk"
set "NDK_HOME=%ANDROID_HOME%\ndk\27.0.12077973"

echo [1/4] Compiling Rust library for Android aarch64...
cargo build --package cloud_sync_tauri --target aarch64-linux-android --lib
if %ERRORLEVEL% NEQ 0 (
    echo Error: Cargo cross-compilation failed.
    exit /b %ERRORLEVEL%
)

echo [2/4] Copying native shared library to Gradle jniLibs...
if not exist "crates\cloud_sync_tauri\gen\android\app\src\main\jniLibs\arm64-v8a" (
    mkdir "crates\cloud_sync_tauri\gen\android\app\src\main\jniLibs\arm64-v8a"
)
copy /y "target\aarch64-linux-android\debug\libcloud_sync_tauri_lib.so" "crates\cloud_sync_tauri\gen\android\app\src\main\jniLibs\arm64-v8a\libcloud_sync_tauri_lib.so" >nul

echo [3/4] Syncing embedded web assets...
if not exist "crates\cloud_sync_tauri\gen\android\app\src\main\assets" (
    mkdir "crates\cloud_sync_tauri\gen\android\app\src\main\assets"
)
xcopy /s /y /q "crates\cloud_sync_tauri\dist\*" "crates\cloud_sync_tauri\gen\android\app\src\main\assets\" >nul

echo [4/4] Assembling Android APK package...
cd crates\cloud_sync_tauri\gen\android
call gradlew.bat assembleArm64Debug
if %ERRORLEVEL% NEQ 0 (
    echo Error: Gradle build failed.
    exit /b %ERRORLEVEL%
)

cd ..\..\..\..
echo.
echo === BUILD COMPLETE ===
echo APK generated at:
echo %CD%\crates\cloud_sync_tauri\gen\android\app\build\outputs\apk\arm64\debug\app-arm64-debug.apk
