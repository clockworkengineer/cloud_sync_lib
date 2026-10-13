# `cloud_sync_tauri`

Cross-platform desktop application shell for `cloud_sync_lib` powered by Tauri 2.0.

---

## 1. Overview

`cloud_sync_tauri` packages the `cloud_sync_ui` web dashboard and the `cloud_sync_daemon` synchronization engine into a native desktop application bundle for Windows, macOS, and Linux.

It provides system tray integration, startup persistence (auto-launch on boot), native window frames, and managed lifecycle orchestration for the background daemon sidecar.

---

## 2. Architecture

```
+-------------------------------------------------------------+
|                     Tauri 2.0 Application                   |
|                                                             |
|  [ Native Desktop Window / WebView ]                        |
|       │                                                     |
|       ▼                                                     |
|  [ In-Process Axum UI Server (cloud_sync_ui) ]              |
|       │ (Serves embedded dashboard on 127.0.0.1:8082)       |
|       │                                                     |
|  [ Auto-Launch System Boot Hook ]                           |
|       │ (Registers app in Windows Registry / macOS Launchd) |
|       │                                                     |
|  [ Sidecar Manager: tauri-plugin-shell ]                    |
|       │                                                     |
+-------│-----------------------------------------------------+
        │ Spawns & monitors child process
        ▼
+-------------------------------------------------------------+
|                     cloud_sync_daemon                       |
|                                                             |
|  [ Core 3-Way Sync Engine + Filesystem Watcher ]            |
+-------------------------------------------------------------+
```

---

## 3. Features

- **Sidecar Lifecycle Management**: Spawns `cloud_sync_daemon` as a managed sidecar child process with argument forwarding (`private_config.toml` or `config.toml`). Captures daemon stdout/stderr streams and relays them cleanly.
- **Embedded Web UI Server**: Starts the Axum HTTP server in a Tokio background task inside the application process (`cloud_sync_ui::start_ui_server`), eliminating external web server dependencies.
- **System Boot Auto-Launch**: Automatically registers the application to start on system boot using the `auto-launch` crate.
- **Zero-Console Windows Subsystem**: Compiles with `#![windows_subsystem = "windows"]` in release mode to prevent console window flicker.

---

## 4. Building & Running

### 4.1 Prerequisites
- Rust stable toolchain
- System WebView2 (pre-installed on Windows 10/11)
- Tauri CLI (optional, but recommended):
  ```bash
  cargo install tauri-cli --version "^2.0"
  ```

### 4.2 Development Mode
Run with Cargo or Tauri CLI:
```bash
cargo run --bin cloud_sync_tauri
# Or using the Tauri CLI:
cargo tauri dev
```

### 4.3 Production Packaging
Build an installer (MSI / EXE on Windows, DMG / App on macOS, AppImage / deb on Linux):
```bash
cargo tauri build
```

The resulting package will bundle the compiled binary, assets, and the `cloud_sync_daemon` sidecar into an installer inside `target/release/bundle/`.
