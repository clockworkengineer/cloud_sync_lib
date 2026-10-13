# State Catalog & Storage Architecture Guide

**Project**: `cloud_sync_lib`  
**Applies to**: `cloud_sync_daemon`, `cloud_sync_core`, `cloud_sync_lib`

---

## 1. Overview

A reliable synchronization engine must track the evolution of files across both the local filesystem and remote cloud storage providers over time. `cloud_sync_lib` implements a **3-way synchronization model** powered by a persistent **State Catalog**.

This guide details:
1. The mathematical foundation of 3-way synchronization.
2. Why state catalogs are strictly isolated in a dedicated directory (`sync_state/`).
3. Binary catalog serialization (`bincode`) and crash-resilient atomic backup mechanisms (`.bak`).
4. Automated legacy catalog migration on startup.
5. Configuration and operational management.

---

## 2. The 3-Way Synchronization Model

### 2.1 The Need for a Baseline State
Simple two-point synchronization (comparing local directly to remote) cannot distinguish whether a file was created on one side or deleted on the other:
- If file `report.pdf` exists locally but not remotely: Was it added locally and needs uploading? Or was it deleted remotely and needs deleting locally?
- If file `notes.txt` exists on both sides with different modification timestamps: Which side represents intentional changes versus concurrent edits?

To resolve these ambiguities deterministically without user intervention, `cloud_sync_lib` maintains a **Base Catalog Snapshot** (the state at the end of the last successful sync cycle):

```
       +-----------------------+
       |   Base Catalog (B)    |
       | (Last Recorded State) |
       +-----------+-----------+
                   │
       ┌───────────┴───────────┐
       ▼                       ▼
+--------------+       +---------------+
|  Local (L)   |       |  Remote (R)   |
| Current Disk |       | Current Cloud |
+--------------+       +---------------+
```

### 2.2 Decision Matrix
During each reconciliation pass, the sync engine evaluates the 3-tuple `(B, L, R)` for every path:

| Base State ($B$) | Local State ($L$) | Remote State ($R$) | Action Taken | Resulting State |
| :--- | :--- | :--- | :--- | :--- |
| $\varnothing$ (None) | Present | $\varnothing$ (None) | **Upload** to Remote | $L = R = B$ |
| $\varnothing$ (None) | $\varnothing$ (None) | Present | **Download** to Local | $L = R = B$ |
| Present | Modified | Unchanged | **Upload** to Remote | $L = R = B$ |
| Present | Unchanged | Modified | **Download** to Local | $L = R = B$ |
| Present | Deleted | Unchanged | **Delete** from Remote | Cleared from $B$ |
| Present | Unchanged | Deleted | **Delete** from Local | Cleared from $B$ |
| Present | Modified | Modified | **Conflict Policy** applied | Resolved per policy |

---

## 3. The Isolated `sync_state/` Directory

### 3.1 Why State Files Must NOT Live in the Watch Directory
In earlier revisions of file synchronization architectures, state catalogs were placed in the root of the monitored folder (e.g., `<watch_dir>/.sync_state_gdrive.bin`). This introduced three severe problems:

1. **Filesystem Watcher Recursion**: Whenever the sync engine updated its catalog after a synchronization cycle, writing the catalog file triggered file modification events in the watcher (`notify` crate). This induced artificial sync loops.
2. **Accidental Cloud Uploads**: Unless complex exclusion filters were perfectly maintained on every backend, hidden state databases risked being synced to remote cloud storage, polluting cloud buckets with internal binary files.
3. **User Confusion & Directory Clutter**: Hidden files appearing in user documents folders cause friction and accidental deletion by end users.

### 3.2 Dedicated Directory Architecture
`cloud_sync_lib` isolates all catalog files into a dedicated `state_directory` (default: `sync_state/` relative to the workspace root):

```
cloud_sync_lib/
├── config.toml
├── daemon.log
├── sync_state/                        <-- Isolated from watched files!
│   ├── .sync_state_Google_Drive.bin
│   ├── .sync_state_Google_Drive.bak
│   ├── .sync_state_Dropbox.bin
│   └── .sync_state_Dropbox.bak
└── watched_folder/                     <-- Clean user directory!
    ├── documents/
    ├── photos/
    └── notes.txt
```

---

## 4. Database Format & Atomic Persistence

### 4.1 Data Structures
Each backend maintains an independent `SyncState` database serialized using `bincode`:

```rust
pub struct SyncItemState {
    pub relative_path: PathBuf,
    pub size: u64,
    pub modified_time: u64,       // Unix timestamp in seconds
    pub checksum: Option<String>, // SHA-256 / provider MD5
    pub is_directory: bool,
}

pub struct SyncState {
    pub items: HashMap<PathBuf, SyncItemState>,
    pub last_sync_time: u64,
    pub backend_version: u32,
}
```

### 4.2 Crash-Resilient Two-Phase Writes (`.bak`)
To ensure that unexpected power outages, crashes, or process terminations never corrupt the state database, writes follow an atomic two-phase commit:

1. **Backup Phase**: If an active `.sync_state_<backend>.bin` already exists, it is copied or renamed to `.sync_state_<backend>.bak`.
2. **Atomic Write Phase**: The new database state is serialized and written to a temporary file (`.sync_state_<backend>.tmp`) followed by an atomic filesystem rename replacing `.sync_state_<backend>.bin`.
3. **Automated Recovery**: Upon startup, if `.sync_state_<backend>.bin` is missing, unreadable, or truncated due to a hard shutdown, the engine automatically attempts loading `.sync_state_<backend>.bak`.

---

## 5. Startup Legacy Migration Engine

When upgrading from older versions where `.sync_state` was located inside the watched folder, `cloud_sync_daemon` automatically migrates legacy databases without requiring manual intervention.

### 5.1 Migration Workflow
At startup, before initiating watcher threads:
1. `sync_engine::migrate_legacy_state_files(&watch_dir, &state_dir).await` is called.
2. The engine scans the root of `watch_dir` for any entries starting with `.sync_state`.
3. Discovered files (e.g., `watched_folder/.sync_state_Google_Drive.bin`) are moved into `state_dir/.sync_state_Google_Drive.bin`.
4. A notification is written to `daemon.log`:
   ```text
   INFO cloud_sync_daemon::sync_engine: Migrated legacy state file from watched folder: .sync_state_Google_Drive.bin -> sync_state/.sync_state_Google_Drive.bin
   ```
5. Subsequent sync cycles run entirely within the isolated `sync_state/` directory.

---

## 6. Configuration & Usage

### 6.1 TOML Configuration
Configure the state catalog location in `config.toml` or `private_config.toml`:

```toml
[sync]
watch_directory = "watched_folder"
interval_seconds = 30

# Path to the dedicated state directory
state_directory = "sync_state"
```

The path can be:
- **Relative**: Evaluated relative to the directory where the daemon process is launched.
- **Absolute**: e.g., `state_directory = "C:/ProgramData/CloudSync/state"` on Windows or `/var/lib/cloudsync/state` on Linux.

### 6.2 CLI Override
Override the state directory dynamically when launching the daemon:

```bash
cargo run --bin cloud_sync_daemon -- --state-dir custom_state_dir
```

### 6.3 Checking State Directory via TCP
Query the active state directory using the daemon's TCP control socket (`127.0.0.1:8081`):

```bash
echo "status" | nc 127.0.0.1 8081
```

**Output**:
```text
State Directory: "C:\\Projects\\cloud_sync_lib\\sync_state"
```

---

## 7. Troubleshooting & Recovery

### 7.1 Forcing a Full Resynchronization
If state desynchronization occurs due to manual offline modifications on multiple devices simultaneously:
1. Stop the daemon:
   ```bash
   echo "stop" | nc 127.0.0.1 8081
   # or click 'Stop Daemon' in the UI
   ```
2. Remove the state database files for the affected backend:
   ```powershell
   Remove-Item sync_state\.sync_state_Google_Drive.*
   ```
3. Restart the daemon.
4. On the next sync cycle, the engine will scan both local and remote filesystems, detecting any discrepancies and reconciling them based on the active `conflict_policy` (e.g., `newer_wins`).
