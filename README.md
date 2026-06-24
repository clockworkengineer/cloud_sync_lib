# Cloud Sync Workspace

This workspace is a Rust-based tool designed to monitor a local watched folder and synchronize file additions, modifications, and deletions in real-time across multiple cloud storage backends.

---

## Workspace Structure

The project is split into two primary components:
1. **[`cloud_sync_lib`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/README.md)**: The core library providing backend integrations, path prefix formatting, simulated/mocked behaviors, and the generic `StorageBackend` trait.
2. **[`cloud_sync_daemon`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_daemon/README.md)**: The CLI daemon wrapper that watches a folder for filesystem events and coordinates synchronization across all enabled backends.

For a detailed look at component interactions, design decisions, and system diagrams, refer to the **[Workspace Architecture Guide](file:///home/robt/projects/cloud_sync_lib/docs/architecture.md)**.

---

## Supported Backends

* **Google Drive**: Full REST API v3 integration with folders support.
* **Dropbox**: Full API v2 integration with custom prefix folders support.
* **OneDrive**: Microsoft Graph REST API integration.

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
   * Refer to the [Google Drive API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/google_drive_setup.md)
   * Refer to the [Dropbox API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/dropbox_setup.md)
   * Refer to the [OneDrive API Setup Guide](file:///home/robt/projects/cloud_sync_lib/docs/onedrive_setup.md)

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
