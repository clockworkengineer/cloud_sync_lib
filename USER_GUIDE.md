# CloudSync User Guide

This guide describes how to configure, run, and interact with the various components of the **CloudSync** suite.

---

## Workspace Executable Overview

The CloudSync suite contains 5 primary executable programs:

| Crate Name | Description | Build Command |
| :--- | :--- | :--- |
| **`cloud_sync_daemon`** | Background sync engine daemon with TCP control interface | `cargo build --bin cloud_sync_daemon` |
| **`cloud_sync_ui`** | Axum Web UI server (port 8082) with SSE live dashboard | `cargo build --bin cloud_sync_ui` |
| **`cloud_sync_tauri`** | Cross-platform Tauri desktop wrapper shell | `cargo build -p cloud_sync_tauri` |
| **`cloud_sync_egui`** | Lightweight native Egui desktop application interface | `cargo build --bin cloud_sync_egui` |
| **`cloud_sync_backup`** | Command-line client for executing one-off sync backups | `cargo build --bin cloud_sync_backup` |

---

## 1. `cloud_sync_daemon` (Core Service)

The daemon runs continuously in the background, listening to local file changes and executing periodic pulls from remote backends.

### Command Line Interface
```powershell
# Start the daemon using default config.toml
cargo run --bin cloud_sync_daemon

# Start using a custom configuration path
cargo run --bin cloud_sync_daemon -- my_config.toml

# Run in single-shot mode (synchronize once and exit immediately)
cargo run --bin cloud_sync_daemon -- --single-shot

# Override conflict resolution policy (choices: rename-local, rename-remote, overwrite, keep-local, keep-remote)
cargo run --bin cloud_sync_daemon -- --conflict-policy overwrite

# Perform a dry-run (simulates sync without modifying local or remote files)
cargo run --bin cloud_sync_daemon -- --dry-run

# Specify directories to exclude from sync
cargo run --bin cloud_sync_daemon -- --exclude "*.tmp" --exclude "build/"
```

### TCP Control Protocol
The daemon listens on TCP port `127.0.0.1:8081` for control commands. You can interact with it using `nc`, `telnet`, or custom scripts:
- `status`: Returns status overview, active backends, current sync state, and error logs.
- `pause`: Temporarily halts background folder monitoring.
- `resume`: Re-enables background sync operations.
- `sync`: Triggers an immediate full bidirectional sync.
- `clear <ProviderName>`: Deletes all files on the specified provider (e.g. `clear MEGA`).
- `subscribe`: Holds the connection open to stream real-time JSON sync events.
- `stop`: Gracefully terminates the background daemon.

---

## 2. `cloud_sync_ui` (Web Dashboard)

A web server running on port `8082` that serves a premium dashboard displaying transfer speeds, active files, and log outputs.

### Usage
1. Start the UI server:
   ```powershell
   cargo run --bin cloud_sync_ui
   ```
2. Open your web browser and navigate to: [http://127.0.0.1:8082](http://127.0.0.1:8082).
3. The dashboard automatically connects to the background daemon using Server-Sent Events (SSE) to update progress bars and log streams in real-time.

---

## 3. `cloud_sync_tauri` (Desktop Application)

An HTML/CSS/JS webview application compiled as a standalone native desktop program wrapping the `cloud_sync_ui` dashboard.

### Usage
- Run in development mode:
   ```powershell
   cargo tauri dev
   ```

---

## 4. `cloud_sync_egui` (Native GUI)

A lightweight GUI interface built using `egui` and `eframe` that interacts directly with the daemon's TCP socket.

### Usage
- Start the egui application:
   ```powershell
   cargo run --bin cloud_sync_egui
   ```

---

## 5. `cloud_sync_backup` (CLI Backup Client)

A dedicated command-line interface helper intended for automated cron jobs or one-off CLI terminal backups.

### Usage
```powershell
# Perform a backup scan using default config
cargo run --bin cloud_sync_backup
```

---

## Advanced Configurations (`config.toml`)

You can customize advanced features directly inside your `config.toml` file:

### Selective Synchronization
Sync only specific folders/paths for a backend by adding `selective_sync` to the provider credentials table:
```toml
[mega_credentials]
enabled = true
sync_mode = "two-way"
destination_folder = "MySyncFolder"
# Only sync "Photos" and "Documents/Work" sub-directories
selective_sync = ["Photos", "Documents/Work"]
```

### Dynamic Bandwidth Scheduler
Schedule rate limit profiles by time of day:
```toml
# Throttle bandwidth to 100KB/s during office hours
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

### Exponential Backoff & Error Recovery
Configure network retry parameters:
```toml
[error_recovery]
max_retries = 5           # Max retry attempts before failing
initial_delay_ms = 500    # Initial backoff wait in milliseconds
multiplier = 2.0          # Wait multiplier per consecutive failure
```
