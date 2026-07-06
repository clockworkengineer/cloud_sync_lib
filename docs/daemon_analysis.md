# Technical Analysis & Feature Recommendations - Cloud Sync Daemon

This document provides a comprehensive technical analysis of the current `cloud_sync_daemon` architecture and recommends new or missing features to improve reliability, security, and utility.

---

## 1. Architectural Strengths

- **Immediate, Event-Driven Sync**: By using the OS event loop (via the `notify` crate), the daemon reacts instantly to local filesystem changes rather than relying on heavy polling loops.
- **Concurrent locking per file/backend**: Spawns independent tokio tasks with per-file, per-backend mutex locks, ensuring concurrent changes to different files don't block each other.
- **Graceful config reloading**: The daemon runs a separate TCP server on port `8081` that allows control actions (status, reload, pause, resume) to be executed without restarting the binary.
- **Dry-run Simulation fallbacks**: Allows offline testing using local directory targets.

---

## 2. Key Limitations & Missing Features

### 1. Exclusion Patterns / Ignore Lists (`.syncignore`) [Implemented]
*   **Status**: Implemented.
*   **Details**: The daemon uses the `ignore` crate to build standard gitignore pattern matchers based on `.syncignore` file contents and the configuration's `exclude` list, skipping excluded files/directories during synchronization and directory scanning.

### 2. Bidirectional / Two-Way Syncing (Remote to Local)
*   **Current state**: Sync is strictly one-way (local to remote). If a file is added or modified on the cloud interface directly, the daemon will not pull it down.
*   **Why it's missing**: True cloud drives require two-way syncing.
*   **Suggestion**: Implement a periodic "pull scan" that lists remote directories, compares modification dates/hashes, and downloads newer files, with conflict resolution logic (e.g., creating `.conflict` files).

### 3. Client-Side Encryption (Zero-Knowledge Sync)
*   **Current state**: Files are uploaded in plaintext.
*   **Why it's missing**: Many users sync private files to public clouds and require zero-knowledge encryption.
*   **Suggestion**: Implement an `EncryptedBackend` wrapper that implements `StorageBackend` and automatically encrypts file bytes (using `aes-gcm` or `chacha20poly1305`) before writing/uploading.

### 4. Bandwidth Rate Limiting [Implemented]
*   **Status**: Implemented.
*   **Details**: Implemented a thread-safe token-bucket rate limiter. Local folder simulation and streams wrap file copies/IO with `copy_rate_limited`, throttling bytes to the maximum upload/download rates defined in `config.toml`.

### 5. Empty Directory Syncing
*   **Current state**: The watcher explicitly skips directory creation events (lines 111-122 of `watcher.rs`).
*   **Why it's missing**: Empty folders created by the user are ignored.
*   **Suggestion**: Extend the `StorageBackend` trait to include `create_folder` and propagate folder creation events.
