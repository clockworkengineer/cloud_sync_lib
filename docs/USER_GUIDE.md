# CloudSync User Guide

This guide describes how to configure, run, and interact with the various components of the **CloudSync** suite.

---

## Workspace Executable Overview

The CloudSync suite contains 5 primary executable programs and convenient Windows launch scripts:

| Component | Description | Build / Run Command | Convenience Launcher |
| :--- | :--- | :--- | :--- |
| **`cloud_sync_daemon`** | Background sync engine with TCP socket interface | `cargo run --bin cloud_sync_daemon` | — |
| **`cloud_sync_ui`** | Axum Web UI server (port 8082) with SSE dashboard | `cargo run --bin cloud_sync_ui` | `scripts/launch_web_ui.{cmd,sh}` |
| **`cloud_sync_egui`** | Native immediate-mode Egui desktop client | `cargo run --bin cloud_sync_egui` | `scripts/launch_egui.{cmd,sh}` |
| **`cloud_sync_tauri`** | Cross-platform Tauri desktop wrapper shell | `cargo run -p cloud_sync_tauri` | `cargo tauri dev` |
| **`cloud_sync_backup`** | Command-line client for one-off CLI backups | `cargo run --bin cloud_sync_backup` | — |

---

## 1. Quick Launch Scripts

Convenient launch scripts are provided in the `scripts/` directory for Windows, Linux, and macOS:

* **Launch Native Desktop GUI**:
  - Windows: `scripts\launch_egui.cmd`
  - Linux / macOS: `./scripts/launch_egui.sh`
* **Launch Web Dashboard**:
  - Windows: `scripts\launch_web_ui.cmd`
  - Linux / macOS: `./scripts/launch_web_ui.sh`

On Windows, these scripts locate precompiled release/debug binaries and launch them in the background using `wscript.exe` without visible command prompts. On Linux and macOS, they start background processes and launch your system's default browser automatically. If binaries have not been compiled yet, they fall back smoothly to `cargo run`.

---

## 2. `cloud_sync_daemon` (Core Service)

The daemon runs continuously in the background, watching local files for modifications and executing periodic reconciliation against remote cloud backends.

### Command Line Interface
```powershell
# Start using default config.toml / private_config.toml
cargo run --bin cloud_sync_daemon

# Start using a custom configuration path
cargo run --bin cloud_sync_daemon -- my_config.toml

# Specify a custom log destination
cargo run --bin cloud_sync_daemon -- --log-file C:\Logs\cloud_sync.log

# Specify a custom isolated state directory (default: sync_state)
cargo run --bin cloud_sync_daemon -- --state-dir custom_state_dir

# Run in single-shot mode (synchronize once across all backends and exit)
cargo run --bin cloud_sync_daemon -- --single-shot

# Override conflict resolution policy (newer_wins, keep_local, keep_remote, manual)
cargo run --bin cloud_sync_daemon -- --conflict-policy newer_wins

# Perform a dry-run (simulates sync without modifying local or remote files)
cargo run --bin cloud_sync_daemon -- --dry-run

# Specify file patterns to exclude from synchronization
cargo run --bin cloud_sync_daemon -- --exclude "*.tmp" --exclude "build/"
```

### TCP Control Protocol
The daemon listens on TCP port `127.0.0.1:8081` (configurable via `CLOUDSYNC_DAEMON_ADDR`). You can interact with it using text commands via PowerShell, `netcat`, or custom scripts:

```powershell
# Send a command using PowerShell
$client = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 8081)
$stream = $client.GetStream()
$writer = New-Object System.IO.StreamWriter($stream)
$reader = New-Object System.IO.StreamReader($stream)
$writer.WriteLine("status")
$writer.Flush()
$reader.ReadToEnd()
$client.Close()
```

Supported TCP commands:
- `status`: Returns running status, watch directory, log file, state directory, active backends, and current transfer activity.
- `get_log`: Streams the full content of the active log file over the socket.
- `sync`: Triggers an immediate bidirectional synchronization pass.
- `pause`: Temporarily halts filesystem event monitoring and scheduled sync passes.
- `resume`: Re-enables automated background synchronization.
- `reload`: Hot-reloads configuration files and reinitializes storage backends.
- `stop`: Gracefully terminates the background daemon.

---

## 3. `cloud_sync_ui` (Web Dashboard)

A web server running on port `8082` that serves an embedded management dashboard with live transfer status, conflict resolution, and log inspection.

### Usage
1. Start the UI server:
   ```bash
   cargo run --bin cloud_sync_ui
   # or run: scripts\launch_web_ui.cmd (Windows) / ./scripts/launch_web_ui.sh (Linux/macOS)
   ```
2. Open your web browser: [http://127.0.0.1:8082](http://127.0.0.1:8082).
3. **Log Stream**: Click **📄 View Log** in the top navigation bar or navigate directly to `http://127.0.0.1:8082/api/logs` to view raw, timestamped daemon logs in your browser.
4. **Conflicts Card**: If concurrent modifications occur, inspect file differences and click **Keep Local** or **Keep Remote** to resolve collisions interactively.

---

## 4. `cloud_sync_egui` (Native Desktop App)

An immediate-mode desktop client built with `egui` and `eframe` that connects directly to the daemon's TCP socket.

### Usage
1. Launch the application:
   ```bash
   scripts\launch_egui.cmd       # Windows
   ./scripts/launch_egui.sh      # Linux / macOS
   # or: cargo run --bin cloud_sync_egui
   ```
2. **Operations Center**: One-click buttons to Start, Stop, Pause, Resume, or trigger an immediate Sync.
3. **Log Viewer**: Click **Logs** to open the in-app log viewer modal with auto-scroll and live refresh. Click **Open in External Editor** to open the log file directly in your system's default text editor without file-locking conflicts.
4. **Provider Pills**: View real-time provider connectivity (`Active`, `Simulated`, or `Failed`).

---

## 5. `cloud_sync_tauri` (Desktop Application)

An HTML/CSS/JS webview wrapper for the `cloud_sync_ui` dashboard compiled into an OS native application with system tray and sidecar daemon management.

```powershell
cargo run -p cloud_sync_tauri
# Or in development mode:
cargo tauri dev
```

---

## 6. Configuration Guide (`config.toml` / `private_config.toml`)

Configure global synchronization options under the `[sync]` table:

```toml
[sync]
watch_directory = "watched_folder"
interval_seconds = 30
conflict_policy = "newer_wins"

# Configurable log file output path
log_file = "daemon.log"

# Dedicated directory for state catalogs (isolated from watch_directory)
state_directory = "sync_state"
```

### Selective Synchronization
Sync only specific folders/paths for a backend:
```toml
[providers.dropbox]
enabled = true
destination_folder = "MySyncFolder"
# Only sync "Photos" and "Documents" sub-directories
selective_sync = ["Photos", "Documents"]
```

### Dynamic Bandwidth Scheduler
Schedule rate limit profiles by time of day:
```toml
# Throttle bandwidth during office hours (09:00 - 17:00)
[[bandwidth_schedule]]
start_time = "09:00"
end_time = "17:00"
max_upload_rate = 100
max_download_rate = 100

# Full speed (unlimited) at night
[[bandwidth_schedule]]
start_time = "17:00"
end_time = "09:00"
max_upload_rate = 0
max_download_rate = 0
```

---

## 7. Additional Technical Documentation

- 📘 [Logging, Observability & Diagnostics Guide](logging_and_diagnostics.md)
- 🗄️ [State Catalog & Storage Architecture Guide](state_and_storage.md)
- 🛠️ [Troubleshooting & Operations Playbook](troubleshooting.md)
- 🏗️ [Architecture Specification](architecture.md)
