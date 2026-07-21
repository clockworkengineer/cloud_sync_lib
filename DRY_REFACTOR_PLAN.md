# DRY Refactor Plan for Cloud Sync Library

This plan outlines concrete refactoring steps to reduce code duplication and improve maintainability across the `cloud_sync_lib` crates.

---

## 1. Implement `ProviderConfig` on `BackendCredentials`

### The Problem
In [crates/cloud_sync_lib/src/providers/mod.rs](file:///c:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/providers/mod.rs), the `BackendCredentials` enum manually matches on all 14 variants for helper methods such as `sync_mode()` and `selective_sync()`. Furthermore, inside `BackendRegistry::build_wrapped()`, matching blocks are duplicated for:
- `sync_mode` (approx. 30 lines)
- `max_upload_rate` (approx. 30 lines)
- `max_download_rate` (approx. 30 lines)
- `encryption_password` (approx. 30 lines)

This leads to ~150 lines of redundant pattern matching boilerplate.

### Refactor Plan
1. Implement the `ProviderConfig` trait directly on `BackendCredentials` in [crates/cloud_sync_lib/src/providers/mod.rs](file:///c:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/providers/mod.rs):
   ```rust
   impl ProviderConfig for BackendCredentials {
       fn common_settings(&self) -> &CommonProviderSettings {
           match self {
               #[cfg(feature = "google_drive")]
               BackendCredentials::GoogleDrive(c) => c.common_settings(),
               #[cfg(feature = "dropbox")]
               BackendCredentials::Dropbox(c) => c.common_settings(),
               #[cfg(feature = "onedrive")]
               BackendCredentials::OneDrive(c) => c.common_settings(),
               #[cfg(feature = "webdav")]
               BackendCredentials::WebDAV(c) => c.common_settings(),
               #[cfg(feature = "s3")]
               BackendCredentials::S3(c) => c.common_settings(),
               #[cfg(feature = "sftp")]
               BackendCredentials::SFTP(c) => c.common_settings(),
               #[cfg(feature = "nextcloud")]
               BackendCredentials::Nextcloud(c) => c.common_settings(),
               #[cfg(feature = "box")]
               BackendCredentials::Box(c) => c.common_settings(),
               #[cfg(feature = "mega")]
               BackendCredentials::Mega(c) => c.common_settings(),
               #[cfg(feature = "azure_blob")]
               BackendCredentials::AzureBlob(c) => c.common_settings(),
               #[cfg(feature = "gcs")]
               BackendCredentials::GCS(c) => c.common_settings(),
               #[cfg(feature = "b2")]
               BackendCredentials::B2(c) => c.common_settings(),
               #[cfg(feature = "pcloud")]
               BackendCredentials::PCloud(c) => c.common_settings(),
               #[cfg(feature = "ipfs")]
               BackendCredentials::IPFS(c) => c.common_settings(),
           }
       }
   }
   ```
2. Remove the manual implementations of `sync_mode` and `selective_sync` on `BackendCredentials`, as they will be automatically inherited via the `ProviderConfig` trait.
3. Clean up `BackendRegistry::build_wrapped()` to call the trait methods directly:
   ```rust
   let sync_mode = creds.sync_mode();
   let max_upload_rate = creds.max_upload_rate();
   let max_download_rate = creds.max_download_rate();
   let encryption_password = creds.encryption_password();
   ```

---

## 2. Eliminate or Utilize Builder Boilerplate

### The Problem
All 14 provider builders (e.g. `GoogleDriveProviderBuilder`, `DropboxProviderBuilder`, etc.) define `timeout` and `custom_headers` fields:
```rust
pub struct GoogleDriveProviderBuilder {
    pub credentials: OAuthCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}
```
However, in all `.build(self)` methods, these parameters are entirely discarded. The HTTP client is built using a global helper `super::utils::build_http_client()`, which ignores the builder configurations.

### Refactor Plan
- **Option A (Simplify/Clean up):** If custom timeouts and headers are not required, remove these fields and their setter methods (`timeout()`, `custom_headers()`) from all 14 builders to reduce code footprint.
- **Option B (Unify/Implement):** Modify `build_http_client` inside [crates/cloud_sync_lib/src/providers/utils.rs](file:///c:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/providers/utils.rs) to accept optional `Duration` and `HeaderMap`, and pass them from the builders:
  ```rust
  pub fn build_http_client(
      timeout: Option<std::time::Duration>,
      headers: Option<reqwest::header::HeaderMap>,
  ) -> reqwest::Client {
      let mut builder = reqwest::Client::builder()
          .timeout(timeout.unwrap_or(std::time::Duration::from_secs(600)))
          .pool_max_idle_per_host(10);
      if let Some(h) = headers {
          builder = builder.default_headers(h);
      }
      builder.build().unwrap_or_else(|_| reqwest::Client::new())
  }
  ```
  Then, update all provider `new` functions and builders to consume and pass these options.

---

## 3. Unify Box OAuth Token Refresh Logic

### The Problem
[crates/cloud_sync_lib/src/providers/box_provider.rs](file:///c:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/providers/box_provider.rs) contains its own duplicate token refresh mechanism and state caching (`CachedToken`, `get_access_token()`). While it does have a specific requirement to update the local configuration files upon refresh (due to Box's token rotation policy), the cache state management and HTTP refresh request can still be delegated or integrated.

### Refactor Plan
1. Modify `OAuthTokenManager` in [crates/cloud_sync_lib/src/providers/utils.rs](file:///c:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/providers/utils.rs) to support an optional callback hook or event listener when a new token is retrieved.
2. Refactor `BoxProvider` to use `OAuthTokenManager` to manage the underlying HTTP refresh and caching, subscribing to the rotation event to safely execute config updates on the local disk.

---

## 4. Standardize Rate Limiting Configuration

### The Problem
Only a subset of providers (Google Drive, Dropbox, OneDrive, WebDAV, Local Simulation) have `with_limiters()` implemented on their structs. More importantly, when constructing storage backends through `BackendRegistry::build_wrapped()`, the backend rate limiters are never initialized; only the wrapping `LocalSimulation` is configured with rate limiters.

### Refactor Plan
1. Move the rate-limiting token bucket generation out of individual provider constructors.
2. Standardize rate-limiting support by applying rate limiting uniformly at the wrapper layer (e.g., inside `SimulatedFallback` or a generic `RateLimitingWrapper`), rather than having redundant logic across some but not all backend implementations.
