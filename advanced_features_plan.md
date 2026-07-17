# Advanced Features & Scaling Plan

This plan details high-value features and scalability improvements to introduce to the `cloud_sync` workspace.

---

## 1. Sync Optimization

### A. Intelligent File Move/Rename Detection
* **Problem**: Currently, when a local file is renamed or moved to another directory, the sync engine treats this as a deletion of the old file and an upload of a new file. This wastes significant bandwidth and time.
* **Proposed Solution**:
  - Implement a move-detection phase during path partitioning.
  - If a file is deleted in one location and added in another, compare their checksums/sizes. If they match, issue a single `rename`/`move` command (for backends that support it) or copy and delete remotely, avoiding re-uploading the file content.

### B. Selective Synchronization
* **Problem**: Users may want to synchronize only specific subfolders with certain backends to save space/bandwidth on limited cloud plans.
* **Proposed Solution**:
  - Add a `selective_sync` list of directory paths to `config.toml` (under provider credentials).
  - Modify the directory scanner to only traverse and yield items matching those paths for the specified backend.

---

## 2. Advanced Scheduling & Traffic Control

### A. Dynamic Rate Limiting & Scheduling
* **Problem**: Static rate limits apply 24/7, restricting bandwidth during off-peak hours.
* **Proposed Solution**:
  - Allow specifying cron-like or time-windowed bandwidth profiles in `config.toml` (e.g., limit to 100KB/s between 09:00 and 17:00, and unlimited at night).
  - Implement a scheduling thread that adjusts rate limiters on active backends dynamically.

### B. Exponential Backoff & Error Recovery
* **Problem**: Transient network issues or rate-limiting responses (HTTP 429) can cause sync operations to fail immediately.
* **Proposed Solution**:
  - Standardize transient error retries using exponential backoff with jitter.
  - Make max retry counts and base backoff delays fully configurable in the settings.

---

## 3. UI & Real-Time Monitoring

### A. Real-Time Event Streaming (SSE)
* **Problem**: UIs must poll the daemon to receive status updates, causing latency and overhead.
* **Proposed Solution**:
  - Implement a Server-Sent Events (SSE) stream or WebSocket endpoint in the control server (`control.rs`).
  - Broadcast transfer status, progress percentages, active bandwidth speeds, and logs in real-time to connecting UIs (Tauri / Egui).
