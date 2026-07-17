# Cloud Sync Backup Daemon

A background daemon that periodically performs unidirectional synchronization (one-way backups) from a source storage provider to a destination storage provider. It leverages the core workspace library (`cloud_sync_lib`) to support backups across local directories and various cloud storage backends.

---

## Key Features

- **Unidirectional Backups**: Keeps a destination directory or cloud storage path synced with a source directory or cloud storage path.
- **Remote-to-Remote Sync**: Supports syncing directly from one cloud provider to another (e.g., Google Drive to Dropbox) using temporary local staging.
- **Smart Change Detection**: Compares checksums if supported by the providers, and falls back to comparing file sizes and modification times (only copying if the source file is strictly newer) to prevent redundant network transfers.
- **Configurable Intervals**: Runs periodically based on a customizable timeout interval.

---

## Configuration

The backup daemon reads its configuration from a TOML file (default is `backup_config.toml`). 

Configure the `[backup]` section to define the source and destination providers, and configure the corresponding credentials sections block as needed:

```toml
[backup]
source_provider = "google_drive"       # e.g., "local", "google_drive", "dropbox", "s3"
source_path = "MySyncFolder"           # Sub-directory prefix on the source provider
destination_provider = "dropbox"
destination_path = "MyBackupFolder"    # Sub-directory prefix on the destination provider
backup_interval_secs = 60              # Interval in seconds between backup checks

[google_credentials]
client_id = "your_google_client_id"
client_secret = "your_google_client_secret"
refresh_token = "your_google_refresh_token"

[dropbox_credentials]
client_id = "your_dropbox_client_id"
client_secret = "your_dropbox_client_secret"
refresh_token = "your_dropbox_refresh_token"
```

### Supported Providers
- **`local`**: Requires setting `source_path` or `destination_path` to a local directory path (e.g., `./watched_folder`).
- **Cloud Providers**: `"google_drive"`, `"dropbox"`, `"onedrive"`, `"webdav"`, `"s3"`, `"sftp"`, `"nextcloud"`, `"mega"`.

---

## Execution

To start the backup daemon, run the binary from the workspace root and pass the configuration file:

```bash
cargo run -p cloud_sync_backup -- backup_config.toml
```

If no configuration file argument is passed, it defaults to looking for `backup_config.toml` in the current directory.
