# `cloud_sync_tauri`

Cross-platform desktop and mobile application shell for `cloud_sync_lib` powered by Tauri 2.0.

---

## 1. Overview

`cloud_sync_tauri` packages the `cloud_sync_lib` synchronization capabilities into a native cross-platform application for Windows, macOS, Linux, and Android.

It provides:
- **Dual Execution Models**:
  - **Desktop (Sidecar or In-Process)**: Can run `cloud_sync_daemon` as a managed sidecar process or execute sync tasks in-process.
  - **Mobile (In-Process Sync)**: Android-compliant in-process engine running inside `tauri::async_runtime` without external process spawns or TCP loopback requirements.
- **Embedded Web Dashboard**: Self-contained web interface bundled inside `dist/index.html` with responsive mobile layout and viewport safe-area support.
- **Direct Tauri IPC Commands**: High-performance native bridge (`status`, `sync`, `pause`, `resume`, `conflicts`, `resolve_conflict`) with graceful fallback to HTTP `/api` endpoints on desktop.
- **Background Sync Scheduler**: Automated in-process sync passes every 30 seconds when operating in mobile/in-process mode.
- **Native Lifecycle**: System tray integration, startup persistence (auto-launch on boot), and native window frames.

---

## 2. Architecture

```
+-------------------------------------------------------------+
|                     Tauri 2.0 Application                   |
|                                                             |
|  [ Native Window / WebView / Android Activity ]             |
|       │                                                     |
|       ▼                                                     |
|  [ Direct Tauri IPC / commands.rs ]                         |
|       │                                                     |
|       ├──► Desktop Mode: Sidecar process or Axum UI Server   |
|       │                                                     |
|       └──► Mobile Mode: In-Process Engine                   |
|                 │                                           |
|                 ▼                                           |
|            [ Periodic Background Sync Worker ]              |
|                 │ (tauri::async_runtime)                    |
|                 ▼                                           |
|            [ cloud_sync_daemon::run_sync_all_backends ]     |
+-------------------------------------------------------------+
```

---

## 3. Features

- **Mobile Ready (`mobile_entry_point`)**: Exports `cloud_sync_tauri_lib` as `cdylib` and `staticlib` with `tauri::mobile_entry_point` for Android NDK integration.
- **Direct IPC Commands**: JavaScript directly triggers Rust commands via `@tauri-apps/api/core` `invoke()`, avoiding localhost network restrictions on Android.
- **In-Process Engine**: Directly executes synchronization passes via `cloud_sync_daemon`'s library interface (`run_sync_all_backends`) and `spawn_periodic_sync_worker`.
- **Desktop Sidecar Support**: On desktop targets, can still spawn `cloud_sync_daemon` as a managed sidecar child process with argument forwarding and relaying.
- **Embedded Web UI Server**: Starts an Axum HTTP server on desktop (`cloud_sync_ui::start_ui_server`) while serving bundled HTML/JS directly on mobile.
- **Zero-Console Windows Subsystem**: Compiles with `#![windows_subsystem = "windows"]` in release mode to prevent console window flicker.

---

## 4. Building & Running

### 4.1 Quick Launch Scripts
- **Windows**: `scripts\launch_tauri.cmd`
- **Linux / macOS**: `./scripts/launch_tauri.sh`

### 4.2 Desktop Development Mode
```bash
cargo run --bin cloud_sync_tauri
# Or using the Tauri CLI:
cargo tauri dev
```

### 4.3 Android Development & Build
For full prerequisites (Android Studio, NDK, Rust targets), see the **[Android Client Guide](../../docs/ANDROID_CLIENT.md)**.

```bash
# Initialize Android project files
cargo tauri android init

# Run on connected Android device or emulator
cargo tauri android dev

# Build release APK / AAB
cargo tauri android build
```

### 4.4 Desktop Production Packaging
Build a desktop installer (MSI / EXE on Windows, DMG / App on macOS, AppImage / deb on Linux):
```bash
cargo tauri build
```

