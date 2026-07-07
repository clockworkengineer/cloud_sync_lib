# Future Features Plan: `cloud_sync_lib`

This document details several high-value future features proposed for the `cloud_sync_lib` codebase, focused on improving performance, reliability, and security.

---

## 1. Global Bandwidth Rate Limiting (REST Providers) [Implemented]

### Background
[rate_limit.rs](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/rate_limit.rs) provides a robust thread-safe `TokenBucket` rate limiter along with `RateLimitedReader` and `RateLimitedStream`. However, it is currently only integrated into the local filesystem simulation.

### Proposed Changes
- **Request Wrapping**: In REST clients (such as Google Drive, Dropbox, OneDrive, WebDAV), wrap the file upload payload inside a `RateLimitedReader` before executing the HTTP POST/PUT request.
- **Response Wrapping**: Wrap the download body stream in a `RateLimitedStream` when downloading files.
- **Configuration**: Expose `max_upload_rate` and `max_download_rate` parameters in the provider credentials and the daemon configuration.

---

## 2. Concurrent/Parallel Sync Engine [Implemented]

### Background
Currently, the daemon's `sync_engine.rs` processes all file synchronization tasks sequentially. This results in poor throughput when syncing large numbers of small files due to cumulative network round-trip times.

### Proposed Changes
- **Task Worker Queue**: Implement a concurrent sync dispatcher in `sync_engine.rs` using `tokio::spawn` or a thread pool.
- **Concurrency Cap**: Make the maximum number of concurrent workers configurable (e.g. defaulting to 4 concurrent sync tasks).
- **Conflict Handling Safety**: Ensure files in the same directory are locked or sequenced properly to prevent concurrent race conditions on metadata updates.

---

## 3. Checksum-Based File Integrity

### Background
Currently, the sync state relies entirely on file `size` and the `local_modified`/`remote_modified` timestamps. If a file gets corrupted during transmission or is modified without changing size/timestamp, it can escape detection.

### Proposed Changes
- **Checksum Metadata**: Add a `checksum` (`Option<String>`) field to `StorageItem`.
- **Backend Hashing**:
  - Implement a standard SHA-256 or MD5 local file hashing routine.
  - Extract the remote file checksum from HTTP headers or API metadata (e.g., S3 ETags, Google Drive `md5Checksum`, Dropbox `content_hash`).
- **Post-Sync Verification**: Verify checksums immediately after uploading/downloading and retry on mismatch.

---

## 4. Transient Error Retries with Exponential Backoff

### Background
Currently, the storage clients fail immediately on any standard connection dropout or rate-limiting response (`HTTP 429` / `HTTP 503`), throwing a `StorageError::Reqwest` or `StorageError::Provider`.

### Proposed Changes
- **Retry Middleware**: Introduce a standard retry mechanism for all HTTP REST request endpoints.
- **Exponential Backoff**: Implement backoff logic (using the `tokio-retry` crate or custom logic) that waits progressively longer between retries, handling transient network drops and rate-limiting responses gracefully.

---

## 5. Glob-based Exclusions (`.syncignore`) [Implemented]

### Background
The watcher and sync engine currently monitor and synchronize every file in the directory root, which can include heavy system directories or temporary files (like `.DS_Store` or `.git/`).

### Proposed Changes
- **Ignore Parser**: Introduce a native pattern matching engine inside `cloud_sync_lib` that parses `.syncignore` or `.gitignore` files.
- **Sync Filtering**: Filter out files matching any matching rules during directory traversals in both local scans and remote file listings.
