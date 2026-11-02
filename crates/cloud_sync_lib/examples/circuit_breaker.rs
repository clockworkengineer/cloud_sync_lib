//! # Circuit Breaker Resilience Example
//!
//! Demonstrates how `CircuitBreakerLayer` isolates chronic cloud provider outages
//! and prevents cascading failures by fast-failing until the remote service recovers.
//!
//! Run with:
//! ```bash
//! cargo run --example circuit_breaker -p cloud_sync_lib
//! ```

use cloud_sync_lib::{
    CircuitBreakerConfig, CircuitBreakerLayer, CloudSyncClient, LayeredBackendExt, StorageBackend,
    StorageCapabilities, StorageError, StorageItem,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Simulated unreliable cloud service.
struct OutageBackend {
    attempts: AtomicU32,
    service_restored: AtomicBool,
}

#[async_trait::async_trait]
impl StorageBackend for OutageBackend {
    fn name(&self) -> &str {
        "remote_cloud_service"
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::NONE
    }

    async fn upload(&self, _local: &Path, _remote: &str) -> Result<(), StorageError> {
        let count = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if self.service_restored.load(Ordering::SeqCst) {
            println!(
                "  -> [OutageBackend] Attempt #{}: Service restored! Request processed.",
                count
            );
            Ok(())
        } else {
            println!(
                "  -> [OutageBackend] Attempt #{}: 503 Service Unavailable / Timeout",
                count
            );
            Err(StorageError::ConnectionFailed(
                "503 Service Unavailable".into(),
            ))
        }
    }

    async fn download(&self, _remote: &str, _local: &Path) -> Result<(), StorageError> {
        Ok(())
    }

    async fn delete(&self, _remote: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn list(&self, _remote: &str) -> Result<Vec<StorageItem>, StorageError> {
        Ok(vec![])
    }
}

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    println!("=== CloudSync: Circuit Breaker Resilience Example ===\n");

    let outage_backend = Arc::new(OutageBackend {
        attempts: AtomicU32::new(0),
        service_restored: AtomicBool::new(false),
    });

    // 1. Configure a circuit breaker that trips after 3 failures and probes after 200ms
    let cb_config = CircuitBreakerConfig {
        failure_threshold: 3,
        recovery_timeout: Duration::from_millis(200),
        probe_success_threshold: 1,
    };

    let protected_backend = outage_backend
        .clone()
        .with_layer(CircuitBreakerLayer::new(cb_config));
    let client = CloudSyncClient::new(protected_backend);

    let dummy_file = Path::new("invoice.pdf");

    // 2. Perform 3 calls that hit the failing backend and trip the breaker
    println!("Phase 1: Sustained outage occurs; calling cloud service 3 times:");
    for i in 1..=3 {
        let res = client.upload(dummy_file, "invoice.pdf").await;
        println!("Call #{}: {}", i, res.unwrap_err());
    }

    // 3. 4th call immediately fast-fails locally without touching the remote server
    println!("\nPhase 2: Circuit breaker is now OPEN; attempting immediate 4th call:");
    let fast_fail_err = client.upload(dummy_file, "invoice.pdf").await.unwrap_err();
    println!("Fast-fail response: {}", fast_fail_err);
    println!(
        "Total requests dispatched to backend: {} (Notice call #4 was intercepted locally!)",
        outage_backend.attempts.load(Ordering::SeqCst)
    );

    // 4. Simulate outage recovery after recovery timeout
    println!("\nPhase 3: Waiting 250ms for circuit recovery window...");
    tokio::time::sleep(Duration::from_millis(250)).await;
    outage_backend
        .service_restored
        .store(true, Ordering::SeqCst);
    println!("Simulated cloud service status: RESTORED.");

    // 5. Next call acts as a probe in HalfOpen state, succeeds, and resets circuit to Closed
    println!("Sending probe call in HalfOpen state:");
    client.upload(dummy_file, "invoice.pdf").await?;
    println!("Probe succeeded! Circuit is back to CLOSED.");

    println!("\n=== Circuit Breaker Resilience Example finished cleanly! ===");
    Ok(())
}
