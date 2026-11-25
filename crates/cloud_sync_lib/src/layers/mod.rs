//! Composable middleware layer architecture for storage backends (OpenDAL-style).
//!
//! Provides intercepting layers for retries, rate limiting, metrics telemetry,
//! logging, and client-side encryption.

pub mod cache;
pub mod circuit_breaker;
pub mod compression;
pub mod logging;
pub mod metrics;
pub mod rate_limit;
pub mod retry;

pub use cache::{CacheLayer, CachedBackend};
pub use circuit_breaker::{CircuitBreakerConfig, CircuitBreakerLayer};
pub use compression::{CompressedBackend, CompressionLayer};
pub use logging::LoggingLayer;
pub use metrics::{MetricsCollector, MetricsLayer, StorageMetrics};
pub use rate_limit::RateLimitLayer;
pub use retry::{RetryConfig, RetryLayer};

use cloud_sync_core::StorageBackend;
use std::sync::Arc;

/// A composable middleware layer that wraps an inner `StorageBackend`.
pub trait StorageLayer: Send + Sync {
    /// Wraps the provided storage backend with this layer.
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend>;
}

/// Helper extension trait allowing `.with_layer(layer)` chaining.
pub trait LayeredBackendExt {
    fn with_layer<L: StorageLayer>(self: Arc<Self>, layer: L) -> Arc<dyn StorageBackend>;
}

impl<T: StorageBackend + 'static> LayeredBackendExt for T {
    fn with_layer<L: StorageLayer>(self: Arc<Self>, layer: L) -> Arc<dyn StorageBackend> {
        layer.layer(self)
    }
}

impl LayeredBackendExt for dyn StorageBackend {
    fn with_layer<L: StorageLayer>(self: Arc<Self>, layer: L) -> Arc<dyn StorageBackend> {
        layer.layer(self)
    }
}

/// Composable middleware layer applying zero-knowledge AES-256-GCM encryption (OCP).
#[derive(Clone)]
pub struct EncryptionLayer {
    password: String,
}

impl EncryptionLayer {
    pub fn new(password: impl Into<String>) -> Self {
        Self {
            password: password.into(),
        }
    }
}

impl StorageLayer for EncryptionLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(crate::providers::EncryptedBackend::new(
            backend,
            &self.password,
        ))
    }
}

/// Composable middleware layer applying transparent large-file chunking (OCP).
#[derive(Debug, Clone, Copy)]
pub struct ChunkerLayer {
    chunk_size_bytes: usize,
}

impl ChunkerLayer {
    pub fn new(chunk_size_bytes: usize) -> Self {
        Self { chunk_size_bytes }
    }
}

impl StorageLayer for ChunkerLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(crate::providers::ChunkerBackend::new(
            backend,
            self.chunk_size_bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_layer_stacking_and_layer_traits() {
        let temp = tempdir().unwrap();
        let local_sim: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            temp.path().to_path_buf(),
            "LayerSim".to_string(),
        ));

        // Test with_layer on dyn StorageBackend and concrete types
        let stacked = local_sim
            .with_layer(LoggingLayer)
            .with_layer(MetricsLayer::default())
            .with_layer(RetryLayer::default())
            .with_layer(CircuitBreakerLayer::default())
            .with_layer(CacheLayer::new(std::time::Duration::from_secs(60)))
            .with_layer(ChunkerLayer::new(1024 * 1024))
            .with_layer(EncryptionLayer::new("secret_password_123"));

        assert!(stacked.name().contains("Encrypted"));
        assert!(stacked
            .capabilities()
            .contains(cloud_sync_core::StorageCapabilities::NONE));
    }
}
