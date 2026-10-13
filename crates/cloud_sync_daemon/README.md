# `cloud_sync_daemon`

A robust background synchronization daemon that watches local folders and orchestrates parallel 3-way synchronization across multiple cloud storage providers.

---

## 1. Overview & How It Works

1. **Filesystem Event Observation**: Monitors local directories using the `notify` cross-platform filesystem watcher.
2. **Debounced Reconciliation**: Coalesces rapid filesystem event bursts to prevent duplicate transfers during large writes or unzips.
3. **State Catalog Isolation**: Maintains 3-way state catalogs (`.sync_state_<backend>.bin` and `.bak`) inside an isolated `sync_state/` directory to prevent watcher loops and cloud pollution.
4. **Non-Blocking Observability**: Emits tracing events to stdout and an asynchronous, non-blocking file sink (`daemon.log`), queryable via TCP `get_log` and HTTP `/api/logs`.
5. **Decoupled TCP Control Socket**: Listens on `127.0.0.1:8081` for external orchestration by `cloud_sync_ui` and `cloud_sync_egui`.
6. **Graceful Fallback**: Automatically switches to in-memory/disk simulation mode if provider credentials are placeholder or absent.
7. **Security & Path Hygiene**: Rejects and sanitizes relative path traversal attempts (`..`, `./`, or absolute path injections).

---

## 2. Command Line Interface (CLI)

```bash
cargo run --bin cloud_sync_daemon -- [CONFIG_FILE] [OPTIONS]
```

### CLI Arguments & Options
| Argument / Flag | Description | Default |
| :--- | :--- | :--- |
| `[CONFIG_FILE]` | Path to configuration TOML file. | `private_config.toml` (if exists), else `config.toml` |
| `--log-file <PATH>` | Custom path for the active log sink. | `"daemon.log"` (or config value) |
| `--state-dir <PATH>` | Custom directory for `.sync_state` catalogs. | `"sync_state"` (or config value) |
| `--single-shot` | Executes one complete sync cycle across all backends and exits immediately. | `false` |
| `--dry-run` | Performs reconciliation and logs planned actions without modifying local or remote files. | `false` |
| `--conflict-policy <POL>` | Overrides conflict resolution policy: `newer_wins`, `keep_local`, `keep_remote`, `manual`. | From config |
| `--exclude <PATTERN>` | Additional glob patterns to exclude from synchronization. | None |
| `--pmu-hook` | Enables low-level Hardware Performance Monitoring Unit sync telemetry. | `false` |

---

## 3. Configuration Reference

The daemon reads configuration from TOML (e.g., `config.toml` or `private_config.toml`):

```toml
[sync]
watch_directory = "./watched_folder"
interval_seconds = 30
conflict_policy = "newer_wins"

# Configurable log file output path
log_file = "daemon.log"

# Dedicated directory for state catalogs (isolated from watch_directory)
state_directory = "sync_state"

# Storage provider credentials
[providers.google_drive]
enabled = true
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"

[providers.dropbox]
enabled = true
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"

[providers.onedrive]
enabled = true
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"

[providers.box]
enabled = true
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"

[providers.webdav]
enabled = false
url = "http://127.0.0.1:8080"
username = "..."
password = "..."

[providers.s3]
enabled = false
bucket = "my-bucket"
region = "us-east-1"
access_key_id = "..."
secret_access_key = "..."
endpoint = "http://127.0.0.1:9000"

[providers.sftp]
enabled = false
host = "127.0.0.1"
port = 22
username = "..."
password = "..."

[providers.nextcloud]
enabled = false
url = "http://127.0.0.1:8080"
username = "..."
app_password = "..."
```

---

## 4. TCP Socket Control Protocol

When running, the daemon listens on `127.0.0.1:8081` (configurable via `CLOUDSYNC_DAEMON_ADDR`). You can interact with it using text commands:

| Command | Response | Description |
| :--- | :--- | :--- |
| `status` | Formatted multi-line text | Reports running state, watch directory, log file, state dir, active providers, and sync progress. |
| `get_log` | `Status: OK\nLog File: "..."\n\n<content>` | Streams the current contents of the active log file. |
| `sync` | `Status: OK` | Initiates an immediate reconciliation pass across all backends. |
| `pause` | `Status: OK` | Pauses filesystem watcher events and scheduled sync cycles. |
| `resume` | `Status: OK` | Resumes automated synchronization. |
| `reload` | `Status: OK` | Reloads configuration files and reinitializes storage backends. |
| `stop` | `Status: OK` | Signals the daemon to shut down cleanly. |

---

## 5. Related Documentation

- [Logging & Diagnostics Guide](../../docs/LOGGING_AND_DIAGNOSTICS.md)
- [State Catalog & Storage Architecture](../../docs/STATE_AND_STORAGE.md)
- [Troubleshooting & Operations Guide](../../docs/TROUBLESHOOTING.md)
- [User Guide](../../docs/USER_GUIDE.md)

---

## Support

If you find this project useful, consider supporting its development:

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/roberttizz1)
