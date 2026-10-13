# Cloud Sync Workspace

A high-performance, modular Rust workspace designed to watch local folders and synchronize file additions, modifications, and deletions in real-time across multiple cloud storage providers with 3-way synchronization, catalog isolation, and end-to-end observability.

---

## Workspace Structure

The workspace is split into modular crates:

1. **[`cloud_sync_lib`](crates/cloud_sync_lib/README.md)**: Core provider client integrations, client-side encryption, rate limiting, and backend factories.
2. **[`cloud_sync_daemon`](crates/cloud_sync_daemon/README.md)**: Background CLI service watching local directories, managing 3-way state catalogs, and exposing TCP control interfaces.
3. **[`cloud_sync_ui`](crates/cloud_sync_ui/README.md)**: Axum-based HTTP web server and REST/SSE API gateway serving an embedded management dashboard.
4. **[`cloud_sync_egui`](crates/cloud_sync_egui/README.md)**: Native GPU-accelerated immediate-mode desktop GUI built on `egui` and `eframe`.
5. **[`cloud_sync_tauri`](crates/cloud_sync_tauri/README.md)**: Tauri 2.0 cross-platform desktop shell wrapping the Web UI and managing daemon sidecars.
6. **[`cloud_sync_backup`](crates/cloud_sync_backup/README.md)**: Unidirectional periodic backup client for one-off CLI or cron-based jobs.
7. **[`cloud_sync_core`](crates/cloud_sync_core/README.md)**: Zero-dependency, `no_std`-compatible storage traits (`StorageBackend`), domain data types, and path security.
8. **[`cloud_sync_std`](crates/cloud_sync_std/README.md)**: Standard library helpers, asynchronous checksum hashers (`compute_sha256`, `compute_md5`), and Ripgrep-based `SyncIgnore` engine.

---

## Quick Launch (Windows)

For daily desktop use on Windows, convenient launch scripts are provided in the `scripts/` directory:

* **Launch Native Desktop App**:
  ```cmd
  scripts\launch_egui.cmd
  ```
* **Launch Web UI Dashboard**:
  ```cmd
  scripts\launch_web_ui.cmd
  ```

These scripts run precompiled binaries windowless via `wscript.exe` without visible command prompts, falling back smoothly to `cargo run` if binaries have not been compiled yet.

---

## Supported Backends

* **Google Drive**: Full REST API v3 integration with folders support.
* **Dropbox**: Full API v2 integration with custom prefix folders support.
* **OneDrive**: Microsoft Graph REST API integration.
* **Box**: Custom app integration via OAuth 2.0 with token rotation and persistence.
* **MEGA**: Client-side encrypted sync utilizing the `mega` library.
* **WebDAV**: Sync to WebDAV-compliant servers.
* **Amazon S3**: AWS S3 or S3-compatible backend storage (e.g., MinIO).
* **SFTP**: Secure File Transfer Protocol sync.
* **Nextcloud**: Sync to Nextcloud instances via WebDAV & OCS APIs.
* **Azure Blob Storage**: Sync to Azure Storage container.
* **Google Cloud Storage (GCS)**: Bucket sync utilizing service account credentials.
* **Backblaze B2**: Backblaze B2 Cloud Storage sync.
* **pCloud**: pCloud storage synchronization via OAuth2 access tokens.
* **IPFS**: InterPlanetary File System sync utilizing pinning services (e.g., Pinata).

---

## Key Features

* **3-Way Synchronization**: Evaluates base catalog snapshot, local disk state, and remote cloud state to execute deterministic updates and handle collisions.
* **State Catalog Isolation**: Keeps `.sync_state_<backend>.bin` and `.bak` catalog files in an isolated `sync_state/` directory, eliminating filesystem watcher loops and accidental cloud pollution.
* **Configurable Logging & Observability**: Non-blocking `tracing-appender` file sink (`daemon.log`), queryable via TCP `get_log`, HTTP `GET /api/logs`, or desktop modal log viewers.
* **Dual GUI Frontends**: Run either the decoupled Axum web dashboard (`cloud_sync_ui`) in any browser or the native desktop GUI (`cloud_sync_egui`).
* **Client-Side Zero-Knowledge Encryption**: Optional client-side AES-256-GCM encryption of file payloads before uploading, with automatic decryption on download. Keys derived via PBKDF2-HMAC-SHA256.
* **Bandwidth Rate-Limiting**: Flexible upload and download bandwidth ceilings using a Token Bucket implementation.
* **Automated Legacy Migration**: Automatically discovers and moves older `.sync_state` databases from watched folders into `sync_state/` on startup.

---

## Documentation Guides

| Guide | Description |
| :--- | :--- |
| 📖 **[User Guide](docs/USER_GUIDE.md)** | Step-by-step instructions for running the daemon, Web UI, Egui app, and CLI commands. |
| 🏗️ **[Architecture Guide](docs/architecture.md)** | Comprehensive architectural diagram, component interactions, and state flow. |
| 📘 **[Logging & Diagnostics Guide](docs/logging_and_diagnostics.md)** | Tracing configuration, non-blocking file sinks, TCP/HTTP log endpoints, and Windows sharing lock handling. |
| 🗄️ **[State Catalog & Storage Guide](docs/state_and_storage.md)** | 3-way synchronization algorithm, catalog isolation in `sync_state/`, atomic `.bak` crash recovery, and migration. |
| 🛠️ **[Troubleshooting & Operations Guide](docs/troubleshooting.md)** | OAuth 2.0 token regeneration, resolving `[Simulated]` mode, port collisions, and database recovery. |

---

## Setup & Configuration

1. **Create Configuration**: Copy the template configuration file:
   ```bash
   cp config.toml.example config.toml
   ```
2. **Configure General & Provider Settings**:
   ```toml
   [sync]
   watch_directory = "watched_folder"
   interval_seconds = 30
   conflict_policy = "newer_wins"
   log_file = "daemon.log"
   state_directory = "sync_state"

   [providers.google_drive]
   enabled = true
   client_id = "..."
   client_secret = "..."
   refresh_token = "..."
   destination_folder = "MySyncFolder"
   ```
3. **Follow API Setup Guides**:
   * Refer to the [Google Drive API Setup Guide](docs/setup/google_drive_setup.md)
   * Refer to the [Dropbox API Setup Guide](docs/setup/dropbox_setup.md)
   * Refer to the [OneDrive API Setup Guide](docs/setup/onedrive_setup.md)
   * Refer to the [Box API Setup Guide](docs/setup/box_setup.md)
   * Refer to the [MEGA Setup Guide](docs/setup/mega_setup.md)
   * Refer to the [WebDAV Setup Guide](docs/setup/webdav_setup.md)
   * Refer to the [S3 Setup Guide](docs/setup/s3_setup.md)
   * Refer to the [SFTP Setup Guide](docs/setup/sftp_setup.md)
   * Refer to the [Nextcloud Setup Guide](docs/setup/nextcloud_setup.md)
   * Refer to the [Azure Blob Storage Setup Guide](docs/setup/azure_blob_storage_setup.md)
   * Refer to the [Google Cloud Storage Setup Guide](docs/setup/google_cloud_storage_setup.md)
   * Refer to the [Backblaze B2 Setup Guide](docs/setup/backblaze_b2_setup.md)
   * Refer to the [pCloud Setup Guide](docs/setup/pcloud_setup.md)
   * Refer to the [IPFS Pinning Service Setup Guide](docs/setup/ipfs_pinning_service_setup.md)

4. **Automated OAuth Token Retrieval**:
   Run the Python helper scripts to retrieve and save refresh tokens:
   * **Google Drive**: `python scripts/get_refresh_token.py`
   * **Dropbox**: `python scripts/get_dropbox_token.py`
   * **OneDrive**: `python scripts/get_onedrive_token.py`
   * **Box**: `python scripts/get_box_token.py`

---

## Running the Applications

### 1. Start the Sync Daemon
```bash
cargo run --bin cloud_sync_daemon
```

### 2. Launch the Desktop Frontends
```cmd
# Native Egui Desktop GUI
scripts\launch_egui.cmd

# Axum Web Dashboard
scripts\launch_web_ui.cmd
```

### 3. Run Workspace Tests
```bash
cargo test --workspace --all-targets
```

---

## Support

If you find this project useful, consider supporting its development:

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/roberttizz1)
