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
- **[`providers/`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/)**: Submodule housing specific client implementations and helpers:
  - [`google_drive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/google_drive.rs): Google Drive REST API integration.
  - [`dropbox.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/dropbox.rs): Dropbox REST API integration. Includes prefix `destination_folder` path handling.
  - [`onedrive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/onedrive.rs): Microsoft OneDrive Graph API integration.
  - [`webdav.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/webdav.rs): WebDAV client integration.
  - [`s3.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/s3.rs): Amazon S3 and S3-Compatible API integration.
  - [`sftp.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/sftp.rs): SFTP client integration.
  - [`nextcloud.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/nextcloud.rs): Nextcloud WebDAV & OCS client integration.
  - [`box_provider.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/box_provider.rs): Box storage API integration.
  - [`mega_provider.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mega_provider.rs): MEGA cloud storage encrypted client integration.
  - [`local_sim.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/local_sim.rs): Shared local fallback simulator (`LocalSimulation`) implementing local folder operations for offline testing.
  - [`utils.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/utils.rs): Helper function `refresh_oauth2_token` for unified, form-encoded OAuth2 access token refresh POST requests.

---

## Local Simulation Fallback

When `OAuthCredentials` (or other provider credentials) are passed as `None` to a provider constructor, the client automatically defaults to **Simulation Mode**, which is powered by the shared `LocalSimulation` struct inside [`local_sim.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/local_sim.rs).

Instead of connecting to remote web APIs, the providers delegate file operations (upload, download, listing, deletion) to this helper. It maps and copies files inside the local simulation directories:
* Google Drive simulated root: `./cloud_simulation/google_drive`
* Dropbox simulated root: `./cloud_simulation/dropbox`
* OneDrive simulated root: `./cloud_simulation/onedrive`
* WebDAV simulated root: `./cloud_simulation/webdav`
* S3 simulated root: `./cloud_simulation/s3`
* SFTP simulated root: `./cloud_simulation/sftp`
* Nextcloud simulated root: `./cloud_simulation/nextcloud`
* Box simulated root: `./cloud_simulation/box`
* MEGA simulated root: `./cloud_simulation/mega`

This layout keeps all providers DRY (Don't Repeat Yourself) while allowing full development and testing without internet access or active cloud tokens.

---

## Testing & Mocking

All providers support dynamic endpoint redirection for automated testing using `wiremock`:
- Google Drive: `.with_endpoints(auth_url, api_url, upload_url)`
- Dropbox: `.with_endpoints(auth_url, api_url, content_url)`

Check [`lib.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/lib.rs) for examples of automated mock HTTP flow tests.
