//! Composable middleware layer architecture for storage backends (OpenDAL-style).
//!
//! Provides intercepting layers for retries, rate limiting, metrics telemetry,
//! logging, and client-side encryption.

pub mod logging;
pub mod metrics;
pub mod rate_limit;
pub mod retry;

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
