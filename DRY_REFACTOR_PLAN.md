# DRY Refactor Plan for Cloud Sync Library (Phase 2)

This document outlines Phase 2 DRY refactoring opportunities identified across `cloud_sync_lib` to further eliminate code duplication and improve maintainability.

---

## 1. Unify Path Prefix Stripping for Provider `list()` Operations

### The Problem
Multiple cloud provider backend implementations (`azure_blob.rs`, `b2.rs`, `gcs.rs`, `ipfs.rs`, `pcloud.rs`, `s3.rs`) duplicate identical boilerplate code in their `list()` methods to strip the `destination_folder` prefix from returned `StorageItem` paths:

```rust
if let Some(ref dest_folder) = self.credentials.common.destination_folder {
    let clean_dest = dest_folder.trim_matches('/');
    if !clean_dest.is_empty() {
        if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
            item_path = stripped.to_path_buf();
        }
    }
}
```

### Refactor Plan
1. Add a shared utility function `strip_destination_prefix(path: &Path, destination_folder: Option<&str>) -> PathBuf` to `cloud_sync_core::path` (and re-export in `providers::utils`).
2. Replace duplicate path-stripping blocks in `azure_blob.rs`, `b2.rs`, `gcs.rs`, `ipfs.rs`, `pcloud.rs`, `s3.rs` with calls to `strip_destination_prefix(&item_path, self.credentials.common.destination_folder.as_deref())`.

---

## 2. Consolidate Custom URL Encoding Utilities

### The Problem
`b2.rs`, `gcs.rs`, `azure_blob.rs`, and `s3.rs` each contain private `url_encode(input: &str) -> String` functions with subtle differences in byte matching (e.g. handling slashes `/` or tildes `~`).

### Refactor Plan
1. Move URL encoding logic to `cloud_sync_core::path` (or `providers::utils`), providing `url_encode(input: &str)` and `url_encode_path(input: &str)` (which preserves slashes).
2. Remove private `url_encode` helper functions from individual backend modules and import the shared utility.

---

## 3. Macroize Provider Builder & Constructor Boilerplate

### The Problem
All 14 provider implementations repeat identical constructor delegations:

```rust
pub fn builder(credentials: Credentials) -> ProviderBuilder {
    ProviderBuilder::new(credentials)
}

pub fn new(credentials: Credentials) -> Self {
    Self::with_client_options(credentials, None, None)
}
```

And all 14 provider builders duplicate identical builder method definitions for `.timeout()` and `.custom_headers()`.

### Refactor Plan
1. Define a macro `impl_provider_builder!(Provider, Builder, Credentials)` in `providers::utils` to generate standard `builder()`, `new()`, `timeout()`, and `custom_headers()` methods.
2. Apply the macro across provider modules to remove ~150 lines of duplicate struct implementation code.

---

## 4. Consolidate Standard Error Response Translation

### The Problem
While `translate_http_error()` is used extensively, several providers (`s3.rs`, `mega_provider.rs`, `sftp.rs`) still convert provider-specific error payloads into `StorageError` using custom `match` or `map_err` blocks that re-implement HTTP status matching.

### Refactor Plan
1. Extend `translate_http_error()` in `providers::utils` to accept an optional custom message or error code extractor.
2. Refactor `s3.rs` and `mega_provider.rs` to route error mapping through `translate_http_error()`.
