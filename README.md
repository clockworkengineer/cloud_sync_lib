# Cloud Sync Workspace

This workspace is a Rust-based tool designed to monitor a local watched folder and synchronize file additions, modifications, and deletions in real-time across multiple cloud storage backends.

---

## Workspace Structure

The workspace is split into several modular components:
1. **[`cloud_sync_lib`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/README.md)**: The core library providing backend integrations, path prefix formatting, simulated/mocked behaviors, client-side encryption, rate limiting, and the generic `StorageBackend` trait.
2. **[`cloud_sync_daemon`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_daemon/README.md)**: The CLI daemon wrapper that watches a folder for filesystem events and coordinates synchronization across all enabled backends.
3. **[`cloud_sync_ui`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_ui/README.md)**: A lightweight Axum-based HTTP server exposing API endpoints to monitor/control the daemon, serving an embedded web dashboard.
4. **[`cloud_sync_tauri`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_tauri/README.md)**: A Tauri-based desktop GUI application that bundles the web UI and manages the daemon as a sidecar.

For a detailed look at component interactions, design decisions, and system diagrams, refer to the **[Workspace Architecture Guide](file:///home/robt/projects/cloud_sync_lib/docs/architecture.md)**.

---

## Supported Backends

* **Google Drive**: Full REST API v3 integration with folders support.
* **Dropbox**: Full API v2 integration with custom prefix folders support.
* **OneDrive**: Microsoft Graph REST API integration.
* **Box**: Custom app integration via OAuth 2.0 with access token rotation/caching and auto-persistence.
* **MEGA**: Client-side encrypted sync utilizing the `mega` library.
* **WebDAV**: Sync to WebDAV-compliant servers.
* **Amazon S3**: AWS S3 or S3-compatible backend storage (like MinIO) sync.
* **SFTP**: Secure File Transfer Protocol sync.
* **Nextcloud**: Sync to Nextcloud instances via WebDAV & OCS APIs.
* **Azure Blob Storage**: Sync to Azure Storage container.
* **Google Cloud Storage (GCS)**: Google Cloud Storage bucket sync utilizing service account credentials.
* **Backblaze B2**: Backblaze B2 Cloud Storage sync.
* **pCloud**: pCloud storage synchronization via OAuth2 access tokens.
* **IPFS**: InterPlanetary File System sync utilizing pinning services (e.g., Pinata).

---

## Key Features

* **Two-Way Synchronization**: Supports both unidirectional (local-to-remote) and bidirectional syncing, including automated local renaming (`*.local-conflict`) for conflict resolution.
* **Client-Side Zero-Knowledge Encryption**: Optional client-side AES-256-GCM encryption of file payloads before uploading, with automatic decryption on download.
* **Bandwidth Rate-Limiting**: Flexible upload and download bandwidth limiting using a Token Bucket implementation.

---

## Setup & Configuration

1. **Create Configuration**: Copy the template configuration file to a private file:
   ```bash
   cp config.toml private_config.toml
   ```
2. **Add Credentials**: Open `private_config.toml` and configure your API credentials and toggles:
   * By default, any backend without credentials falls back to a local folder simulation.
   * Toggle any backend on/off using the `enabled = true/false` parameter under its credentials block.
3. **Follow API Setup Guides**:
   * Refer to the [Google Drive API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/google_drive_setup.md)
   * Refer to the [Dropbox API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/dropbox_setup.md)
   * Refer to the [OneDrive API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/onedrive_setup.md)
   * Refer to the [Box API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/box_setup.md)
   * Refer to the [MEGA Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/mega_setup.md)

4. **Automated OAuth Token Retrieval**:
   For backends requiring OAuth 2.0 (Google Drive, Dropbox, OneDrive, Box), you can easily generate and save refresh tokens by running the provided Python helper scripts after entering your `client_id` and `client_secret` in `private_config.toml`:
   * **Google Drive**: `python3 scripts/get_refresh_token.py`
   * **Dropbox**: `python3 scripts/get_dropbox_token.py`
   * **OneDrive**: `python3 scripts/get_onedrive_token.py`
   * **Box**: `python3 scripts/get_box_token.py`
   * Refer to the [WebDAV Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/webdav_setup.md)
   * Refer to the [S3 Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/s3_setup.md)
   * Refer to the [SFTP Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/sftp_setup.md)
   * Refer to the [Nextcloud Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/nextcloud_setup.md)
   * Refer to the [Azure Blob Storage Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/azure_blob_storage_setup.md)
   * Refer to the [Google Cloud Storage Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/google_cloud_storage_setup.md)
   * Refer to the [Backblaze B2 Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/backblaze_b2_setup.md)
   * Refer to the [pCloud Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/pcloud_setup.md)
   * Refer to the [IPFS Pinning Service Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/setup/ipfs_pinning_service_setup.md)

---

## Execution

### Run the Daemon
Start the synchronization daemon by passing the configuration path:
```bash
cargo run --bin cloud_sync_daemon -- private_config.toml
```

### Run Workspace Tests
Run unit, mock HTTP (using `wiremock`), and real integration tests:
```bash
cargo test --all
```
