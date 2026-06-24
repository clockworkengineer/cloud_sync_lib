# `cloud_sync_lib`

A modular Rust library providing clean abstractions and clients to interface with cloud storage backends.

---

## Core Abstractions

All providers implement the `StorageBackend` trait defined in [`traits.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/traits.rs):

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;
}
```

---

## Crate Layout

- **[`traits.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/traits.rs)**: Core trait definitions, list item metadata (`StorageItem`), and error types (`StorageError`).
- **[`providers/`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/)**: Submodule housing specific client implementations:
  - [`google_drive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/google_drive.rs): Google Drive REST API integration.
  - [`dropbox.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/dropbox.rs): Dropbox REST API integration. Includes prefix `destination_folder` path handling.
  - [`onedrive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/onedrive.rs): Microsoft OneDrive Graph API integration.

---

## Local Simulation Fallback

When `OAuthCredentials` are passed as `None` to a provider constructor, the client automatically defaults to **Simulation Mode**. 

Instead of connecting to remote web APIs, it simulates file operations (upload, download, listing, deletion) inside a local workspace folder:
* Google Drive simulated root: `./cloud_simulation/google_drive`
* Dropbox simulated root: `./cloud_simulation/dropbox`
* OneDrive simulated root: `./cloud_simulation/onedrive`

This enables development and testing without internet access or valid tokens.

---

## Testing & Mocking

All providers support dynamic endpoint redirection for automated testing using `wiremock`:
- Google Drive: `.with_endpoints(auth_url, api_url, upload_url)`
- Dropbox: `.with_endpoints(auth_url, api_url, content_url)`

Check [`lib.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/lib.rs) for examples of automated mock HTTP flow tests.
