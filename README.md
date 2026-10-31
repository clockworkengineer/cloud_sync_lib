# Cloud Sync Workspace

[![CI](https://github.com/clockworkengineer/cloud_sync_lib/actions/workflows/ci.yml/badge.svg)](https://github.com/clockworkengineer/cloud_sync_lib/actions/workflows/ci.yml)
[![Version: 0.2.0](https://img.shields.io/badge/version-0.2.0-orange.svg)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tests: 96 Passed](https://img.shields.io/badge/tests-96%20passed-brightgreen.svg)](#3-run-workspace-tests)

A high-performance, modular Rust workspace designed to watch local folders and synchronize file additions, modifications, and deletions in real-time across multiple cloud storage providers with 3-way synchronization, catalog isolation, and end-to-end observability.

---

## Workspace Structure

The repository is organized as an 8-crate workspace strictly adhering to SOLID principles and clean separation of concerns:

1. **[`cloud_sync_lib`](crates/cloud_sync_lib/README.md)**: High-level consumer client facade (`CloudSyncClient`, `CloudSyncBuilder`), layered middleware pipeline, `DynamicBackendRegistry`, and zero-copy `BufferPool`.
2. **[`cloud_sync_core`](crates/cloud_sync_core/README.md)**: Zero-dependency, `no_std`-compatible domain types, granular storage role traits (`StorageReader`, `StorageWriter`, `DirectoryLister`, `ServerSideOperations`), error taxonomy, `NormalizedPath` security newtype, and `StateStore` trait (`PostcardStateStore` CSTX v1).
3. **[`cloud_sync_daemon`](crates/cloud_sync_daemon/README.md)**: Background synchronization engine and OS daemon service watching local directories via `notify`, listening on TCP port 8081, managing state catalogs, and providing diagnostic verification binaries.
4. **[`cloud_sync_ui`](crates/cloud_sync_ui/README.md)**: Decoupled Axum-based HTTP web server and REST/SSE API gateway (:8082) serving an embedded management dashboard.
5. **[`cloud_sync_egui`](crates/cloud_sync_egui/README.md)**: Native GPU-accelerated immediate-mode desktop GUI built on `egui` and `eframe`.
6. **[`cloud_sync_tauri`](crates/cloud_sync_tauri/README.md)**: Tauri 2.0 cross-platform desktop and mobile shell with in-process synchronization engine and direct IPC commands.
7. **[`cloud_sync_backup`](crates/cloud_sync_backup/README.md)**: Unidirectional periodic backup client for one-off CLI or cron-based jobs.
8. **[`cloud_sync_std`](crates/cloud_sync_std/README.md)**: Standard library helpers, asynchronous checksum hashers (`compute_sha256`, `compute_md5`), and Ripgrep-based `SyncIgnore` engine.

---

## Quick Launch Scripts

Convenient launch scripts for the frontends are provided in the `scripts/` directory:

* **Tauri Desktop & Mobile Interface (`cloud_sync_tauri`)**:
  - Windows: `scripts\launch_tauri.cmd`
  - Linux / macOS: `./scripts/launch_tauri.sh`
* **Native Desktop GUI (`cloud_sync_egui`)**:
  - Windows: `scripts\launch_egui.cmd`
  - Linux / macOS: `./scripts/launch_egui.sh`
* **Web UI Dashboard (`cloud_sync_ui`)**:
  - Windows: `scripts\launch_web_ui.cmd`
  - Linux / macOS: `./scripts/launch_web_ui.sh`

* **Android Mobile APK Builder (`cloud_sync_tauri`)**:
  - Windows: `scripts\build_android.cmd`
  - Linux / macOS: `./scripts/build_android.sh`

On Windows, the launch scripts run precompiled binaries windowless without visible command prompts, falling back smoothly to `cargo run` if binaries have not been compiled yet. On Linux and macOS, they start background processes and open your system's default browser automatically.

For Android mobile architecture, permissions, and deployment workflows, see the **[Android Client Guide](docs/ANDROID_CLIENT.md)**.

---

## Supported Backends

* **Google Drive**: Full REST API v3 integration with folders support.
* **Dropbox**: Full API v2 integration with custom prefix folders support.
* **OneDrive**: Microsoft Graph REST API integration.
* **Box**: Custom app integration via OAuth 2.0 with token rotation and persistence.
* **MEGA**: Client-side encrypted sync utilizing the `mega` library, with automatic semantic mapping and simulation fallback for `EBLOCKED (-16)` verification holds.
* **WebDAV**: Sync to WebDAV-compliant servers.
* **Amazon S3**: AWS S3 or S3-compatible backend storage (e.g., MinIO).
* **SFTP**: Secure File Transfer Protocol sync.
* **Nextcloud**: Sync to Nextcloud instances via WebDAV & OCS APIs.
* **Azure Blob Storage**: Sync to Azure Storage container.
* **Google Cloud Storage (GCS)**: Bucket sync utilizing service account credentials.
* **Backblaze B2**: Backblaze B2 Cloud Storage sync.
* **pCloud**: pCloud storage synchronization via OAuth2 access tokens.
* **IPFS**: InterPlanetary File System sync utilizing pinning services (e.g., Pinata).

---

## Key Features

* **Ergonomic Client Facade**: `CloudSyncClient` and `CloudSyncBuilder` provide a simple, unified API for file operations across all providers.
* **Layered Middleware Pipeline**: OpenDAL-style composable layers (`RetryLayer`, `CircuitBreakerLayer`, `RateLimitLayer`, `LoggingLayer`, `MetricsLayer`).
* **3-Way Synchronization**: Evaluates base catalog snapshot, local disk state, and remote cloud state to execute deterministic updates and resolve conflicts.
* **State Catalog Isolation**: Keeps `.sync_state_<backend>.bin` (CSTX v1 format with CRC32 integrity check) and `.bak` catalog files in an isolated `sync_state/` directory.
* **Path Traversal Defense**: Strong path sanitization through `NormalizedPath` eliminating directory traversal, null-byte injection, Alternate Data Streams, and DOS device names.
* **Configurable Logging & Observability**: Non-blocking `tracing-appender` file sink (`daemon.log`), queryable via TCP `get_log`, HTTP `GET /api/logs`, or desktop modal log viewers.
* **Dual GUI Frontends**: Run either the decoupled Axum web dashboard (`cloud_sync_ui`) in any browser or the native desktop GUI (`cloud_sync_egui`).
* **Client-Side Zero-Knowledge Encryption**: Optional client-side AES-256-GCM authenticated chunk streaming encryption with PBKDF2-HMAC-SHA256 (100,000 iterations) and memory zeroization (`zeroize`).
* **Bandwidth Rate-Limiting**: Flexible upload and download bandwidth ceilings using a Token Bucket implementation.
* **Standalone Diagnostic Verifiers**: Rapid credential and connectivity validation via `check_backends` and provider-specific test binaries (`test_mega`, `test_box`, etc.).

---

## Documentation Guides

| Guide | Description |
| :--- | :--- |
| 📖 **[User Guide](docs/USER_GUIDE.md)** | Step-by-step instructions for running the daemon, Web UI, Egui app, and CLI commands. |
| 🏗️ **[Architecture Guide](docs/ARCHITECTURE.md)** | Comprehensive 8-crate topology, layered middleware pipeline, state flow, and component interactions. |
| 📘 **[Client API Guide](docs/CLIENT_API.md)** | `CloudSyncClient`, `CloudSyncBuilder`, middleware configuration, buffer pools, and code examples. |
| 🔒 **[Security & Reliability Guide](docs/SECURITY_AND_RELIABILITY.md)** | Client-side AEAD encryption, memory zeroization, `NormalizedPath` defenses, full jitter backoff, and circuit breakers. |
| 🗄️ **[State Catalog & Storage Guide](docs/STATE_AND_STORAGE.md)** | 3-way synchronization algorithm, catalog isolation in `sync_state/`, `PostcardStateStore` CSTX v1 format, and atomic crash recovery. |
| 📊 **[Logging & Diagnostics Guide](docs/LOGGING_AND_DIAGNOSTICS.md)** | Tracing spans, non-blocking file sinks, TCP/HTTP log endpoints, and diagnostic checklist. |
| 🔍 **[Diagnostics & Verifiers Reference](docs/DIAGNOSTICS_AND_VERIFIERS.md)** | `check_backends` CLI, standalone provider verifiers (`test_*`), and contract compliance testing. |
| 🧩 **[Extending & Testing Guide](docs/EXTENDING_AND_TESTING.md)** | Custom provider implementations, granular traits, custom middleware layers, and compliance suites. |
| 🛠️ **[Troubleshooting & Operations Guide](docs/TROUBLESHOOTING.md)** | OAuth renewal, MEGA `EBLOCKED (-16)` resolution, circuit breaker states, and port conflict remedies. |

---

## Setup & Configuration

1. **Create Configuration**: Copy the template configuration file:
   ```bash
   cp config.toml.example config.toml
   ```
2. **Configure General & Provider Settings**: Add credentials to `private_config.toml` or `config.toml`.
3. **Verify Connectivity**:
   ```bash
   cargo run --bin check_backends -p cloud_sync_daemon
   ```
4. **Launch Daemon & Interface**:
   ```bash
   scripts\launch_egui.cmd # Windows
   # or
   ./scripts/launch_egui.sh # Linux / macOS
   ```
