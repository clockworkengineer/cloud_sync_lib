# `cloud_sync_lib`

A modular Rust library providing clean abstractions and clients to interface with cloud storage backends.

---

## Core Abstractions

All providers implement the `StorageBackend` trait defined in [`traits.rs`](../cloud_sync_core/src/traits.rs):

```rust
#[async_trait]
pub trait StorageBackend:
    StorageInspector
    + StorageReader
    + StorageWriter
    + StorageDeleter
    + DirectoryLister
    + FolderOps
    + Send
    + Sync
{
    fn name(&self) -> &str;
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError>;
}
```

---

## Crate Layout

- **[`traits.rs`](../cloud_sync_core/src/traits.rs)**: Core trait definitions, list item metadata (`StorageItem`), sync policies (`SyncPolicy`), and error types (`StorageError`).
- **[`providers/`](src/providers/)**: Submodule housing concrete storage provider implementations and virtual composites:
  - [`google_drive.rs`](src/providers/google_drive.rs): Google Drive REST API integration with chunked resumable upload sessions.
  - [`dropbox.rs`](src/providers/dropbox.rs): Dropbox REST API integration with session commit batching and pagination loops.
  - [`onedrive.rs`](src/providers/onedrive.rs): Microsoft OneDrive Graph API integration with large upload sessions.
  - [`webdav.rs`](src/providers/webdav.rs): WebDAV client integration (RFC 4918).
  - [`s3.rs`](src/providers/s3.rs): Amazon S3 and S3-Compatible API integration with AWS v4 signatures.
  - [`sftp.rs`](src/providers/sftp.rs): SFTP client integration with SSH key and password authentication.
  - [`nextcloud.rs`](src/providers/nextcloud.rs): Nextcloud WebDAV & OCS client integration.
  - [`azure_blob.rs`](src/providers/azure_blob.rs): Azure Blob Storage REST API integration.
  - [`gcs.rs`](src/providers/gcs.rs): Google Cloud Storage JSON API integration.
  - [`b2.rs`](src/providers/b2.rs): Backblaze B2 REST API integration.
  - [`pcloud.rs`](src/providers/pcloud.rs): pCloud REST API integration.
  - [`ipfs.rs`](src/providers/ipfs.rs): IPFS Pinning Service REST API integration.
  - [`box_provider.rs`](src/providers/box_provider.rs): Box storage API integration.
  - [`mega_provider.rs`](src/providers/mega_provider.rs): MEGA cloud storage encrypted client integration.
  - [`ftp.rs`](src/providers/ftp.rs): FTP (RFC 959) and FTPS (RFC 4217) passive mode integration.
  - [`http.rs`](src/providers/http.rs): HTTP directory server integration with HTML & JSON autoindex parsing.
  - [`smb.rs`](src/providers/smb.rs): SMB/CIFS network share integration with atomic file staging.
  - [`union.rs`](src/providers/union.rs): Multi-Cloud Union virtual backend (`MirrorAll` RAID-1, `FirstAvailable` failover, `RoundRobin` quota pooling).
  - [`chunker.rs`](src/providers/chunker.rs): Large-file chunking backend with SHA-256 manifests and transparent download reassembly.
  - [`local_sim.rs`](src/providers/local_sim.rs): Shared local fallback simulator (`LocalSimulation`) implementing local folder operations for offline testing.
  - [`encryption.rs`](src/providers/encryption.rs): Client-side zero-knowledge encryption decorator (AES-256-GCM with PBKDF2-HMAC-SHA256).
  - [`utils.rs`](src/providers/utils.rs): Helpers for OAuth2, HTTP error response mapping, transient retry timing tests.
- **[`layers/`](src/layers/)**: OpenDAL-style composable onion middleware:
  - `CompressionLayer`: Pure-Rust `CLZ1` lossless compression with incompressible data bypass.
  - `CacheLayer`: In-memory metadata and directory listing cache with write-through invalidation.
  - `RetryLayer`: Truncated exponential backoff with AWS Full Jitter.
  - `CircuitBreakerLayer`: Fast failout during sustained provider outages.
  - `RateLimitLayer`: Independent upload and download bandwidth throttling using Token Buckets.
  - `MetricsLayer`: High-resolution operation counters, transferred byte totals, and latency distributions.
  - `LoggingLayer`: Structured OpenTelemetry tracing spans.
- **Advanced Engines**:
  - [`cloud_transfer.rs`](src/cloud_transfer.rs): Direct cloud-to-cloud migration and replication (`transfer_between_backends`).
  - [`snapshot.rs`](src/snapshot.rs): Content-addressable point-in-time incremental cloud backups (`SnapshotEngine`).
  - [`dedupe.rs`](src/dedupe.rs): Remote storage duplicate file scanner and resolver (`DedupeStrategy`).
  - [`analytics.rs`](src/analytics.rs): Storage hierarchy metrics, file extension breakdown, and directory tree builder.
  - [`buffer_pool.rs`](src/buffer_pool.rs): Zero-copy recycled byte buffers for large-stream I/O.

---

## Backend Registry (Dynamic Instantiation)

The library provides `DynamicBackendRegistry`, `BackendRegistry`, and `BackendCredentials` inside the [`providers/`](src/providers/mod.rs) module to enable dynamic backend configuration parsing and instantiation:

```rust
use cloud_sync_lib::{BackendRegistry, BackendCredentials};

// Instantiate any backend dynamically by passing its matching enum variant
let provider = BackendRegistry::build(BackendCredentials::S3(s3_credentials));
```

---

## Local Simulation Fallback

When `OAuthCredentials` (or other provider credentials) are passed as `None` to a provider constructor, the client automatically defaults to **Simulation Mode**, which is powered by the shared `LocalSimulation` struct inside [`local_sim.rs`](src/providers/local_sim.rs).

Instead of connecting to remote web APIs, the providers delegate file operations (upload, download, listing, deletion) to this helper. It maps and copies files inside the local simulation directories:
* Google Drive: `./cloud_simulation/google_drive`
* Dropbox: `./cloud_simulation/dropbox`
* OneDrive: `./cloud_simulation/onedrive`
* WebDAV: `./cloud_simulation/webdav`
* S3: `./cloud_simulation/s3`
* SFTP: `./cloud_simulation/sftp`
* Nextcloud: `./cloud_simulation/nextcloud`
* Azure Blob: `./cloud_simulation/azure_blob`
* GCS: `./cloud_simulation/gcs`
* B2: `./cloud_simulation/b2`
* pCloud: `./cloud_simulation/pcloud`
* IPFS: `./cloud_simulation/ipfs`
* Box: `./cloud_simulation/box`
* MEGA: `./cloud_simulation/mega`
* FTP: `./cloud_simulation/ftp`
* HTTP: `./cloud_simulation/http`
* SMB: `./cloud_simulation/smb`

This layout keeps all providers DRY (Don't Repeat Yourself) while allowing full development and testing without internet access or active cloud tokens.

---

## Testing & Mocking

All providers support dynamic endpoint redirection for automated testing using `wiremock`:
- Google Drive: `.with_endpoints(auth_url, api_url, upload_url)`
- Dropbox: `.with_endpoints(auth_url, api_url, content_url)`

Check [`lib.rs`](src/lib.rs) for examples of automated mock HTTP flow tests.
Real integration tests automatically skip gracefully when configuration is missing or tokens are expired.

---

## Support

If you find this project useful, consider supporting its development:

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/roberttizz1)

[Buy Me a Coffee](https://buymeacoffee.com/roberttizz1)
