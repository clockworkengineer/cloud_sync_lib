# `cloud_sync_egui`

Native immediate-mode desktop application for `cloud_sync_lib` built with `egui` and `eframe`.

---

## 1. Overview

`cloud_sync_egui` provides a standalone native desktop GUI interface for managing the `cloud_sync_daemon` background process without requiring a web browser or Node.js runtime.

It interacts directly with the daemon's TCP control socket (`127.0.0.1:8081`), providing real-time status telemetry, control actions (Start, Stop, Pause, Resume, Sync Now, Reload), and an embedded log viewer modal.

---

## 2. Features

- **Pure Rust Native Desktop Client**: Built using `egui 0.27` and `eframe` for cross-platform rendering (DirectX / OpenGL / Metal).
- **Zero-Console Windows Launching**: Configured with `#![windows_subsystem = "windows"]` and `CREATE_NO_WINDOW` flags to ensure no background console windows appear.
- **Asynchronous Tokio Bridge**: UI thread remains smooth at 60 FPS while background socket commands and log fetches run concurrently on a Tokio runtime.
- **Embedded Log Viewer**:
  - In-app modal viewer with auto-scroll and manual refresh.
  - One-click **Open in External Editor** button delegating to PowerShell `Start-Process` (with automatic fallback to `notepad.exe` or `xdg-open`), opening log files without file-sharing locks.
- **Provider Status Pills**: Color-coded badges indicating active, offline, and simulated backends.
- **Offline Self-Start**: If the daemon is not running, the application displays an offline indicator with a single-click button to start the daemon in the background.

---

## 3. Architecture

```
+-------------------------------------------------------+
|                   CloudSyncApp (egui)                 |
|                                                       |
|  [ eframe::App Update Loop (60 FPS) ]                 |
|       │                                               |
|       ▼                                               |
|  [ Tokio Background Runtime ]                         |
|       │                                               |
|       ▼                                               |
|  [ TCP Socket Client ]                                |
+-------│-----------------------------------------------+
        │ (127.0.0.1:8081)
        ▼
+-------------------------------------------------------+
|                 cloud_sync_daemon                     |
|                                                       |
|  [ TCP Control Listener: "status", "sync", "get_log"] |
+-------------------------------------------------------+
```

---

## 4. How to Run

### 4.1 Using the Zero-Console Windows Launcher (Recommended)
From the workspace root, run:
```cmd
scripts\launch_egui.cmd
```

### 4.2 Using Cargo
```bash
cargo run --bin cloud_sync_egui
```

### 4.3 Environment Variables
| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `CLOUDSYNC_DAEMON_ADDR` | `127.0.0.1:8081` | Address of the daemon TCP control socket. |

---

## 5. Keyboard & Interaction Shortcuts

- **Refresh**: Status updates automatically every 2 seconds.
- **Open Logs**: Click **Logs** in the Operations Center card.
- **Open Config**: Click the active configuration path in the status card to open it directly in your registered text editor.
