# Client API Guide & Examples

The `cloud_sync_lib` crate provides an ergonomic, high-level client facade (`CloudSyncClient`) and builder (`CloudSyncBuilder`) that encapsulate storage providers, connection pools, and middleware layers behind a clean, unified API.

---

## 1. Overview & Core Concepts

Instead of directly instantiating low-level provider structs or wrestling with manual token refresh logic, consumers interact with the `CloudSyncClient`:

- **Unified Interface**: Standard operations (`upload`, `download`, `delete`, `list`, `create_folder`, `rename`, `copy`) work identically across all 13 cloud storage providers and local simulations.
- **Layered Onion Middleware**: Composable decorators (`RetryLayer`, `CircuitBreakerLayer`, `RateLimitLayer`, `LoggingLayer`, `MetricsLayer`) wrap the storage backend without modifying provider code.
- **Dynamic Provider Registry**: Dynamically instantiate providers by string identifier (`"google_drive"`, `"dropbox"`, `"mega"`, `"s3"`, etc.) and JSON configuration.
- **Zero-Copy Buffer Pool**: Low-overhead byte buffer reuse via `BufferPool` to eliminate heap fragmentation during large file transfers.

---

## 2. Using `CloudSyncBuilder`

The `CloudSyncBuilder` provides fluent configuration for providers, middleware, and resilience parameters.

### Basic Setup with Local Simulation
For offline development and unit tests:

```rust,no_run
use cloud_sync_lib::{CloudSyncClient, StorageError};
use std::path::Path;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    let client = CloudSyncClient::builder()
        .with_local_sim("cloud_simulation", "local_backup")
        .with_retry(3, Duration::from_millis(100), Duration::from_secs(2))
        .with_logging()
        .with_metrics()
        .build()?;

    // Upload local file to cloud
    client.upload(Path::new("local_doc.txt"), "remote_folder/doc.txt").await?;

    // Download file back
    client.download("remote_folder/doc.txt", Path::new("restored_doc.txt")).await?;

    Ok(())
}
```

### Dynamic Provider Setup
For production setups reading credentials from configuration files or environment variables:

```rust,no_run
use cloud_sync_lib::{CloudSyncClient, StorageError};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    let provider_config = json!({
        "client_id": "oauth-client-id",
        "client_secret": "oauth-client-secret",
        "refresh_token": "oauth-refresh-token"
    });

    let client = CloudSyncClient::builder()
        .with_provider("google_drive", provider_config, PathBuf::from("./cloud_simulation"))
        .with_retry(3, Duration::from_millis(500), Duration::from_secs(5))
        .with_circuit_breaker(5, Duration::from_secs(30))
        .with_rate_limits(10 * 1024 * 1024, 20 * 1024 * 1024) // 10MB/s upload, 20MB/s download
        .with_logging()
        .with_metrics()
        .build()?;

    let items = client.list("").await?;
    for item in items {
        println!("Remote item: {:?} (size: {} bytes)", item.path, item.size);
    }

    Ok(())
}
```

---

## 3. Middleware Layers

Layers wrap any `Arc<dyn StorageBackend>` using the `with_layer` method from `LayeredBackendExt`:

```rust,no_run
use cloud_sync_lib::layers::{CircuitBreakerConfig, CircuitBreakerLayer, LayeredBackendExt, RetryConfig, RetryLayer};
use cloud_sync_core::StorageBackend;
use std::sync::Arc;
use std::time::Duration;

fn wrap_backend(backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
    let retry_cfg = RetryConfig {
        max_attempts: 3,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(3),
    };

    let cb_cfg = CircuitBreakerConfig {
        failure_threshold: 5,
        recovery_timeout: Duration::from_secs(15),
        probe_success_threshold: 2,
    };

    backend
        .with_layer(CircuitBreakerLayer::new(cb_cfg))
        .with_layer(RetryLayer::new(retry_cfg))
}
```

### Layer Catalog

| Layer | Configuration | Purpose |
| :--- | :--- | :--- |
| `CircuitBreakerLayer` | `CircuitBreakerConfig { failure_threshold, recovery_timeout, probe_success_threshold }` | Fails fast during sustained cloud provider outages, preventing request pileups and retry storms. Transitions across `Closed`, `Open`, and `HalfOpen` states. |
| `RetryLayer` | `RetryConfig { max_attempts, base_delay, max_delay }` | Automatically retries transient errors (`is_transient()`) such as HTTP 429, 500, 502, 503, 504 using full-jitter exponential backoff. Non-transient errors (e.g. 401 Unauthorized, 404 Not Found) fail immediately. |
| `RateLimitLayer` | `TokenBucket` for upload & download | Regulates maximum bandwidth consumption (bytes per second) to prevent network saturation. |
| `MetricsLayer` | `MetricsCollector` | Thread-safe collection of total bytes transferred, request counts, error counts, and latency percentiles. |
| `LoggingLayer` | OpenTelemetry span integration | Emits structured tracing events (`storage.upload`, `storage.download`, `storage.delete`, `storage.list`) capturing paths, byte sizes, and duration. |

---

## 4. Zero-Copy Buffer Pool

High-throughput transfers utilize the `BufferPool` to recycle memory buffers and minimize allocation overhead:

```rust,no_run
use cloud_sync_lib::BufferPool;

let pool = BufferPool::new(64 * 1024, 16); // 64KB buffers, capacity of 16
let mut buffer = pool.acquire();

// Write or read data
buffer.as_mut_slice()[..4].copy_from_slice(b"SYNC");

// Automatically zeroed / recycled back into the pool when dropped
drop(buffer);
```

---

## 5. Walkthrough of Runnable Examples

The `crates/cloud_sync_lib/examples/` directory contains complete, standalone examples demonstrating real-world usage patterns:

### 1. Basic Synchronization (`basic_sync.rs`)
- **Path**: `crates/cloud_sync_lib/examples/basic_sync.rs`
- **Command**: `cargo run --example basic_sync -p cloud_sync_lib`
- **Demonstrates**: Setting up a sandbox environment, building a client with local simulation, and executing `upload`, `list`, and `download`.

### 2. Circuit Breaker Resilience (`circuit_breaker.rs`)
- **Path**: `crates/cloud_sync_lib/examples/circuit_breaker.rs`
- **Command**: `cargo run --example circuit_breaker -p cloud_sync_lib`
- **Demonstrates**: Simulating a cloud service outage, observing the circuit breaker trip to `Open` state to fast-fail subsequent requests, waiting for the recovery timeout, and recovering through `HalfOpen` probe successes.

### 3. Custom Provider Registration (`custom_provider.rs`)
- **Path**: `crates/cloud_sync_lib/examples/custom_provider.rs`
- **Command**: `cargo run --example custom_provider -p cloud_sync_lib`
- **Demonstrates**: Implementing the `StorageBackend` trait for a custom in-memory store, defining a `BackendFactory`, registering it into `DynamicBackendRegistry`, and creating the provider dynamically by name.

### 4. Resumable Upload & Rate Limiting (`resumable_upload.rs`)
- **Path**: `crates/cloud_sync_lib/examples/resumable_upload.rs`
- **Command**: `cargo run --example resumable_upload -p cloud_sync_lib`
- **Demonstrates**: Creating multi-megabyte payloads, applying bandwidth rate limits (e.g. 5 MB/s), and verifying transferred byte counts.

---

## 6. Error Handling Best Practices

All client methods return `Result<T, StorageError>`. Inspect semantic predicates to drive recovery logic:

```rust,no_run
use cloud_sync_lib::{CloudSyncClient, StorageError};
use std::path::Path;

async fn sync_file(client: &CloudSyncClient, local: &Path, remote: &str) {
    match client.upload(local, remote).await {
        Ok(()) => println!("Upload succeeded"),
        Err(err) if err.is_auth() => {
            eprintln!("Authentication expired or blocked: {}", err);
            // Prompt user for re-authentication or check browser credentials
        }
        Err(err) if err.is_not_found() => {
            eprintln!("Remote destination path not found: {}", err);
        }
        Err(err) if err.is_transient() => {
            eprintln!("Transient network error (exhausted retries): {}", err);
        }
        Err(err) => {
            eprintln!("Permanent storage error: {}", err);
        }
    }
}
```

