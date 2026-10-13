# Logging, Observability & Diagnostics Guide

**Project**: `cloud_sync_lib`  
**Applies to**: `cloud_sync_daemon`, `cloud_sync_ui`, `cloud_sync_egui`

---

## 1. Overview

`cloud_sync_lib` provides a multi-tier observability pipeline that records system operations, sync engine reconciliation cycles, network requests, conflict detections, and errors.

The observability system supports:
1. **Configurable Log Destinations**: Log output can be routed to stdout, a designated local file, or both simultaneously.
2. **Non-Blocking File I/O**: File logging operates through an asynchronous background worker thread using `tracing-appender`, preventing disk I/O latency from stalling file watchers or network transfers.
3. **Multi-Interface Access**: Logs can be inspected directly on disk, streamed over TCP socket commands, queried via an HTTP REST endpoint (`GET /api/logs`), or viewed live inside the native desktop application (`cloud_sync_egui`).
4. **Cross-Platform File Sharing**: Log files remain accessible for concurrent inspection by external text editors (such as Notepad, VS Code, or `tail`) while the daemon continues appending log records.

---

## 2. Configuration & Activation

### 2.1 Configuration File (`config.toml` / `private_config.toml`)

Log file path configuration is set in the daemon's TOML configuration under the `[sync]` table:

```toml
[sync]
watch_directory = "watched_folder"
interval_seconds = 30
conflict_policy = "newer_wins"
# Configurable log file path (relative to daemon working directory or absolute)
log_file = "daemon.log"
# State catalog directory (isolated from watch_directory)
state_directory = "sync_state"
```

### 2.2 CLI Overrides

The log file can be overridden at startup using the `--log-file` command-line argument:

```bash
# Relative path
cargo run --bin cloud_sync_daemon -- --log-file custom_daemon.log

# Absolute path
cargo run --bin cloud_sync_daemon -- --log-file C:\Logs\cloud_sync.log
```

If neither the CLI flag nor the TOML setting is specified, the daemon defaults to `"daemon.log"` in the current working directory.

### 2.3 Precedence Rules

When resolving the active log file path, the daemon applies the following precedence:
1. **CLI Flag**: `--log-file <PATH>`
2. **TOML Configuration**: `log_file = "<PATH>"` in the active config file
3. **Default**: `"daemon.log"`

### 2.4 Log Level Filtering (`RUST_LOG`)

Log verbosity is controlled via the standard `RUST_LOG` environment variable:

```bash
# Windows PowerShell
$env:RUST_LOG="cloud_sync_daemon=debug,cloud_sync_lib=trace,info"
cargo run --bin cloud_sync_daemon

# Linux / macOS
RUST_LOG=cloud_sync_daemon=debug,info cargo run --bin cloud_sync_daemon
```

If `RUST_LOG` is unset, the daemon defaults to `info` level logging.

---

## 3. Architecture & Internal Pipeline

```
  +-------------------------------------------------------------+
  |                   Cloud Sync Daemon                         |
  |                                                             |
  |  [ tracing::info! / warn! / error! / debug! ]               |
  |                           │                                 |
  |                           ▼                                 |
  |             tracing_subscriber::registry                    |
  |                │                       │                    |
  |                ▼                       ▼                    |
  |          fmt::layer()            fmt::layer()               |
  |          (Stdout / TTY)        (Non-blocking Worker)        |
  |                                        │                    |
  |                                        ▼                    |
  |                             tracing_appender::rolling::never|
  |                                        │                    |
  +----------------------------------------│--------------------+
                                           │
                                           ▼
                                 Active Log File (Disk)
                                  e.g., daemon.log
                                           │
         ┌─────────────────────────────────┼────────────────────────────────┐
         │                                 │                                │
         ▼                                 ▼                                ▼
   Direct Disk                       Daemon TCP Socket                 Web UI Server
   Inspection                     (cmd: "get_log\n")                (GET /api/logs)
 (VS Code, Notepad,                        │                                │
   tail, Get-Content)                      ▼                                ▼
                                    cloud_sync_egui                   Web Browser /
                                   (Log Viewer Modal)                 Embedded Tab
```

### 3.1 Non-Blocking Worker Architecture
In `crates/cloud_sync_daemon/src/main.rs`, the daemon initializes the tracing subscriber with two layers:
1. **Stdout Layer**: Formats and prints colorized output to standard output when running in interactive terminal sessions.
2. **File Appender Layer**: Wraps `tracing_appender::rolling::never(directory, &file_name)` in `tracing_appender::non_blocking`. This channels log entries across an in-memory queue to a dedicated worker thread, ensuring file disk writes do not block Tokio asynchronous tasks.

### 3.2 Canonicalized Path Sanitization
On Windows, file canonicalization prepends `\\?\` (Extended-Length Path Syntax). The daemon sanitizes this prefix before logging and reporting paths to UIs, ensuring user-facing diagnostics display clean Windows paths like `C:\Projects\cloud_sync_lib\daemon.log`.

---

## 4. Querying & Consuming Logs

### 4.1 Direct File Inspection
Because the log file is flushed continuously, standard system tools can tail or inspect it:

```powershell
# PowerShell
Get-Content -Path daemon.log -Tail 50 -Wait

# Git Bash / Linux / macOS
tail -f daemon.log
```

### 4.2 TCP Socket Control Protocol (`get_log`)
The daemon control socket (listening by default on `127.0.0.1:8081`) responds to the `get_log` text command:

```bash
# Using netcat / ncat
echo "get_log" | nc 127.0.0.1 8081
```

**Response Format**:
```text
Status: OK
Log File: "C:\Projects\cloud_sync_lib\daemon.log"

2026-10-13T12:00:00.123456Z  INFO cloud_sync_daemon: Starting Cloud Sync Daemon...
2026-10-13T12:00:00.124567Z  INFO cloud_sync_daemon: Active log file: daemon.log
2026-10-13T12:00:00.125678Z  INFO cloud_sync_daemon: Sync state directory: "sync_state"
...
```

If the log file cannot be read, the daemon returns:
```text
Status: Error
Could not read log file 'daemon.log': The system cannot find the file specified. (os error 2)
```

### 4.3 Web UI REST API (`GET /api/logs`)
The decoupled Axum Web UI (`cloud_sync_ui`) exposes `GET /api/logs`:
- **When Daemon is Online**: The web server sends `get_log` over the TCP socket, extracts the log payload, and streams it with `Content-Type: text/plain; charset=utf-8`.
- **When Daemon is Offline**: The web server detects socket closure and reads the local log file directly from disk, returning recent offline log history.
- **Accessing via Browser**: Open `http://127.0.0.1:8082/api/logs` in any browser or click **📄 View Log** in the Web UI dashboard top bar.

### 4.4 Native Desktop UI (`cloud_sync_egui`)
The immediate-mode desktop app provides dual log capabilities:
1. **In-App Modal Viewer**: Click **Logs** in the Operations card to open a scrollable, searchable dialog with real-time log refresh.
2. **External Editor Delegation**: Click **Open in External Editor** inside the modal. On Windows, this launches a non-blocking process:
   ```powershell
   powershell -NoProfile -WindowStyle Hidden -Command "Start-Process -FilePath 'C:\Projects\cloud_sync_lib\daemon.log'"
   ```
   If PowerShell fails, it falls back immediately to `notepad.exe`. On Unix platforms, it delegates to `xdg-open`.

---

## 5. Windows File-Sharing & Lock Diagnostics

### 5.1 The Sharing Violation Issue
On Windows, opening a file for writing can result in `ERROR_SHARING_VIOLATION` (`os error 32`) if other processes attempt to read or write without compatible sharing modes (`FILE_SHARE_READ | FILE_SHARE_WRITE`).

### 5.2 How `cloud_sync_lib` Avoids Lock Contention
- `tracing-appender` opens files allowing read sharing.
- External editor launching delegates through Windows Shell (`Start-Process`), prompting the registered system text editor (VS Code, Notepad++, Notepad) to open the file in read-shared mode.
- When querying via HTTP or TCP, the daemon and web UI read files asynchronously with Tokio (`tokio::fs::read_to_string`), ensuring zero interference with the logging worker thread.

---

## 6. Diagnostic Checklist

When investigating sync anomalies, look for the following signatures in `daemon.log`:

| Log Signature | Meaning | Remediation |
| :--- | :--- | :--- |
| `INFO ... Active log file: ...` | Successful startup and logger initialization | Verify expected directory location. |
| `WARN ... Falling back to Simulated backend ...` | Provider credentials missing or placeholder | Check `config.toml` / `private_config.toml`. Run token generation script. |
| `INFO ... Migrated legacy state file ...` | Older state file successfully moved into `sync_state/` | Normal automated migration. |
| `WARN ... Conflict detected on ...` | File modified concurrently on both local and remote | Inspect `conflict_policy` or resolve in Web UI. |
| `ERROR ... Rate limit exceeded (429) ...` | Cloud provider API rate limit encountered | Backoff retry will occur automatically; check provider quotas. |
