# DRY Refactor Plan for Cloud Sync Library

This document outlines the multi-phase DRY (Don't Repeat Yourself) refactoring roadmap across the `cloud_sync_lib` workspace to eliminate code duplication, streamline backend provider implementations, and improve long-term code maintainability.

---

## Completed Phases

### Phase 1 [COMPLETED]
1. **Unified `ProviderConfig` Trait:** Implemented on `BackendCredentials` across all providers (`commit 03801c2`).
2. **HTTP Client Pooling & Options:** Plumbed `timeout` and `custom_headers` across all HTTP storage providers (`commit c11f7db`).
3. **Box OAuth Token Refresh:** Centralized token refresh & rotation callback logic into `OAuthTokenManager` (`commit 650a504`).
4. **Rate Limiting Wrapper:** Created generic `RateLimitingBackend` wrapper and integrated it across backends (`commit 06623fb`).

### Phase 2 [COMPLETED]
1. **Unify Path Prefix Stripping:** Added `strip_destination_prefix()` to `cloud_sync_core::path` and applied across all providers (`commit f2b73be`).
2. **Consolidate URL Encoding Utilities:** Added RFC 3986 `url_encode()` and `url_encode_path()` to `cloud_sync_core::path` and removed duplicate private helpers (`commit 3342f3a`).
3. **Macroize Provider Builder & Constructor Boilerplate:** Implemented `impl_provider_builder!` macro across all 14 providers (`commit c8e5b33`).
4. **Consolidate Error Response Translation:** Added `translate_status_code_error()` helper and refactored S3 / HTTP status mappings (`commit ade88e8`).

---

## Phase 3 Refactor Plan

### 1. Unify Configuration Models & Parsing Across Workspace Crates
- **Problem:** `cloud_sync_backup/src/config.rs` duplicates all 14 provider credential optional fields (`google_credentials`, `dropbox_credentials`, `s3_credentials`, etc.) and root directory path definitions line-for-line from `cloud_sync_daemon/src/config.rs`.
- **Refactor Plan:**
  1. Extract common credential struct fields and root path configurations into a shared `AppConfig` / `StorageCredentialsConfig` struct in `cloud_sync_core::config` or `cloud_sync_lib`.
  2. Refactor `cloud_sync_daemon` and `cloud_sync_backup` to embed the shared configuration struct, removing ~100 lines of duplicated struct field definitions.

### 2. Macroize Standalone CLI Verifier Binaries
- **Problem:** Standard CLI diagnostic binaries (`test_box.rs`, `test_mega.rs`, `test_nextcloud.rs`, `test_s3.rs`, `test_sftp.rs`, `test_webdav.rs`) duplicate ~50 lines of boilerplate CLI entry points, config loading, credential validation, and diagnostics invocation.
- **Refactor Plan:**
  1. Define a `define_verifier_binary!(provider_name, feature, CredentialsType, ProviderType, config_field)` macro in `crates/cloud_sync_daemon/src/bin/common.rs`.
  2. Reduce each test binary down to a single declarative macro invocation.

### 3. Centralize Storage Backend Factory (`create_backend`)
- **Problem:** Multiple modules match on provider name/type strings to instantiate `StorageBackend` trait objects and wrap them with `RateLimitingBackend` or `EncryptedBackend`.
- **Refactor Plan:**
  1. Implement a unified backend factory function `cloud_sync_lib::create_backend(config: &ProviderConfig)` or `BackendFactory::from_config(&AppConfig)` that constructs and wraps requested storage backends dynamically.
  2. Refactor `cloud_sync_daemon::sync_engine` and `cloud_sync_backup` to instantiate backends via the centralized factory function.

### 4. Standardize Async File Upload Buffer Streams
- **Problem:** Several non-HTTP/custom protocol providers (`sftp.rs`, `mega_provider.rs`) re-implement custom file chunk reading and Tokio runtime blocking tasks for local-to-remote file streams.
- **Refactor Plan:**
  1. Extract a shared `read_file_chunks(local_path: &Path, buffer_size: usize)` helper into `providers::utils`.
  2. Refactor SFTP and MEGA providers to consume the shared chunk reader helper.
