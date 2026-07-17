# Production-Ready Enhancement & Professionalization Plan

This plan outlines features, diagnostics, and architectural improvements to elevate `cloud_sync_lib` and the sync daemon to a production-grade, professional-tier utility.

---

## 1. Advanced Sync & Conflict Policies

### A. Configurable Conflict Resolution Strategies
* **Current State**: Hardcoded to rename local conflicting files to `<file>.local-conflict` and download the remote version.
* **Proposed Enhancement**:
  - Introduce a `ConflictPolicy` enum in `cloud_sync_core`:
    ```rust
    pub enum ConflictPolicy {
        RenameLocal,   // Default: keep local as .conflict, pull remote
        RenameRemote,  // Keep remote as .conflict, push local
        KeepNewer,     // Automatically choose the file with the newer mtime
        KeepLocal,     // Overwrite remote with local changes
        KeepRemote,    // Overwrite local with remote changes
    }
    ```
  - Expose this via a CLI option (`--conflict-policy`) and the configuration file (`config.toml`).

### B. Dry-Run Mode (`--dry-run`)
* **Proposed Enhancement**:
  - Implement a dry-run flag across the CLI and sync engine.
  - When active, the sync engine executes all change-detection logic and prints precisely what operations (uploads, downloads, creations, deletions, renames) *would* happen, without performing any filesystem writes or cloud API mutating requests.

### C. Dynamic Command-Line Exclusions
* **Proposed Enhancement**:
  - Add `--exclude <glob>` command-line arguments to the daemon.
  - Dynamically append these patterns to the `.syncignore` builder at runtime to ignore specific directories or file extensions temporarily.

---

## 2. Diagnostics & Enterprise Logging

### A. Structured JSON Logging
* **Proposed Enhancement**:
  - Add support for structured JSON logging (configurable via `--log-format json` or `log_format = "json"` in config).
  - This allows enterprise monitoring tools (Datadog, ELK, Splunk) to parse sync activities, status reports, and throughput metrics programmatically.

### B. Sync Progress & Stats Reporter
* **Proposed Enhancement**:
  - Keep track of transfer stats during a sync run (e.g., total bytes transferred, speed in KB/s, files synced/failed/skipped).
  - Print a clean summary table upon completion of single-shot runs or at intervals in daemon mode.

---

## 3. Storage & Metadata Enhancements

### A. Partitioned State Storage
* **Proposed Enhancement**:
  - Instead of saving all synced file metadata in a single massive `.bin` postcard catalog (which degrades performance on large directories), partition the state catalog by top-level subdirectory or bucket.
  - Or, introduce a lightweight sqlite/sled database engine behind a feature flag for enterprise deployments.

### B. Pre-calculated Local Hash Cache
* **Proposed Enhancement**:
  - Compute and cache local file hashes in the sync state. During the next sync pass, if the file size and mtime are unchanged, avoid recalculating the hash (e.g. SHA-256 or Dropbox hash) to drastically reduce CPU and disk read overhead.
