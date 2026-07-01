# `cloud_sync_daemon`

A background CLI daemon that watches a local folder and automatically synchronizes all changes (creations, edits, deletions) to enabled cloud storage providers.

---

## How It Works

1. **Monitors Filesystem Events**: Uses the `notify` crate to watch a configured local folder.
2. **Debounces Event Bursts**: Prevents duplicate uploads for rapid modification events.
3. **Synchronizes Parallelly**: Spawns concurrent asynchronous tasks to upload/delete files on each active cloud backend.
4. **Handles Offline Fallback**: Safely defaults to local directory simulation if a backend's credentials are not configured.

---

## Configuration

The daemon reads configuration from a TOML file (e.g. `private_config.toml`):

```toml
watch_directory = "./watched_folder"
google_drive_root = "./cloud_simulation/google_drive"
dropbox_root = "./cloud_simulation/dropbox"
onedrive_root = "./cloud_simulation/onedrive"

[google_credentials]
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"
enabled = true

[dropbox_credentials]
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"
enabled = true

[box_credentials]
client_id = "..."
client_secret = "..."
refresh_token = "..."
destination_folder = "MySyncFolder"
enabled = true

[mega_credentials]
email = "..."
password = "..."
destination_folder = "MySyncFolder"
enabled = true
```

---

## Enabling / Disabling Backends

You can easily enable or disable individual backends inside your credentials blocks using the `enabled` key:
* **`enabled = true`** (or omitting the field): The backend is active. If credentials are correct, it syncs to the cloud; otherwise, it logs errors.
* **`enabled = false`**: Bypasses the backend completely. The provider is not initialized, and no sync attempts are made.

Under the hood, the daemon uses unified helper functions (like `is_enabled` and `is_mega_enabled`) to deduplicate config verification. This prevents redundant pattern matching and config checking before initializing each provider instance.

If a credentials section is omitted entirely, the daemon defaults to **Simulation Mode** for that provider, synchronizing to a folder inside the local directory.
