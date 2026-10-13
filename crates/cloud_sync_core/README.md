# `cloud_sync_core`

Foundational zero-dependency and `no_std`-compatible abstractions, traits, and data structures for `cloud_sync_lib`.

---

## 1. Overview

`cloud_sync_core` defines the core interfaces and domain models that all storage providers, synchronization algorithms, and daemons in the `cloud_sync_lib` ecosystem implement.

By decoupling the trait definitions and state structures from network runtimes (Tokio, Reqwest) and standard I/O, `cloud_sync_core` can be compiled into embedded targets, WebAssembly, kernel/driver modules, or high-performance CLI utilities.

---

## 2. Key Abstractions & Traits

### 2.1 `StorageBackend`
The primary asynchronous interface that all cloud providers (Google Drive, Dropbox, OneDrive, Box) and local filesystem drivers implement:

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;
    async fn list(&self, remote_folder: &str) -> Result<Vec<StorageItem>, StorageError>;
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError>;
    fn name(&self) -> &str;
}
```

### 2.2 Granular Trait Decomposition
For systems requiring fine-grained capabilities:
- **`StorageReader`**: Pure read and download operations.
- **`StorageWriter`**: File uploads and deletions.
- **`DirectoryLister`**: Traversal and recursive hierarchy enumeration.
- **`FolderOps`**: Creating and managing directories.
- **`ChecksumOps`**: Hashing and metadata integrity verification.

---

## 3. Core Data Types

### 3.1 Domain Models
- **`StorageItem`**: Metadata container representing a file or directory (path, size, modification timestamp, checksum, `is_directory`).
- **`StorageError`**: Canonical error enumeration (`NotFound`, `PermissionDenied`, `RateLimitExceeded`, `NetworkError`, `Conflict`, `Other`).
- **`FileState`**: Checksum and timestamp snapshot for tracking 3-way synchronization states.
- **`SyncState`**: In-memory catalog mapping paths to recorded `FileState` entries.

### 3.2 Policies
- **`ConflictPolicy`**: Enum defining how to handle concurrent edits (`NewerWins`, `KeepLocal`, `KeepRemote`, `Manual`).
- **`SyncMode`**: Synchronization direction (`Bidirectional`, `UploadOnly`, `DownloadOnly`).
- **`SyncPolicy`**: Encapsulates conflict handling, retry backoff strategies, and bandwidth limits.

### 3.3 Path Normalization
- **`NormalizedPath`**: Enforces forward slashes, sanitizes leading/trailing slashes, and protects against directory traversal vulnerabilities (`../`).

---

## 4. Features & Compilation Targets

```toml
[dependencies.cloud_sync_core]
version = "0.2.0"
default-features = false # Enables pure no_std mode!
features = ["std"]      # Enables std file system and tokio integration
```

- **`default = ["std"]`**: Enables standard library features, standard error implementations, and filesystem wrappers.
- **`no_std` Mode**: Disables `std` allocations except for `alloc`. Postcard serialization and `hashbrown` are used for embedded compatibility.
