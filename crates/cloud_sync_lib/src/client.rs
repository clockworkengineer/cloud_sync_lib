//! # Cloud Sync Client Facade
//!
//! Provides a unified, ergonomic, and intuitive entry point for interacting with
//! cloud storage providers, performing file operations, and executing synchronization tasks.

use crate::layers::{
    CacheLayer, CircuitBreakerConfig, CircuitBreakerLayer, CompressionLayer, LayeredBackendExt,
    LoggingLayer, MetricsLayer, RateLimitLayer, RetryConfig, RetryLayer,
};
use crate::providers::{
    ChunkerBackend, DynamicBackendRegistry, EncryptedBackend, UnionBackend, UnionPolicy,
};
use crate::rate_limit::TokenBucket;
use crate::transport::HttpTransport;
use cloud_sync_core::{StorageBackend, StorageError, StorageItem};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// High-level summary of a directory synchronization operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    /// Number of files uploaded.
    pub uploaded: usize,
    /// Number of files downloaded.
    pub downloaded: usize,
    /// Number of files deleted remotely or locally.
    pub deleted: usize,
    /// Number of unresolvable conflicts encountered.
    pub conflicts: usize,
    /// Detailed error messages encountered during execution.
    pub errors: Vec<String>,
}

/// Unified, ergonomic client for cloud storage operations.
///
/// Encapsulates a configured `StorageBackend` along with middleware layers
/// (retry, rate limiting, logging, metrics) behind a clean, intuitive API.
///
/// # Example
/// ```rust,no_run
/// use cloud_sync_lib::client::CloudSyncClient;
/// use std::path::Path;
///
/// # async fn doc_example() -> Result<(), cloud_sync_lib::StorageError> {
/// let client = CloudSyncClient::builder()
///     .with_local_sim("cloud_simulation", "local")
///     .with_retry(3, std::time::Duration::from_millis(500), std::time::Duration::from_secs(5))
///     .build()?;
///
/// client.upload(Path::new("report.pdf"), "documents/report.pdf").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct CloudSyncClient {
    backend: Arc<dyn StorageBackend>,
}

impl CloudSyncClient {
    /// Initiates construction of a new `CloudSyncClient` via fluent builder.
    pub fn builder() -> CloudSyncBuilder {
        CloudSyncBuilder::new()
    }

    /// Wraps an existing, fully configured `StorageBackend` directly.
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self { backend }
    }

    /// Returns a reference to the underlying storage backend.
    pub fn backend(&self) -> &Arc<dyn StorageBackend> {
        &self.backend
    }

    /// Returns the provider name (e.g. "google_drive", "dropbox", "local").
    pub fn provider_name(&self) -> &str {
        self.backend.name()
    }

    /// Uploads a local file to the cloud destination path.
    pub async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        self.backend.upload(local_path, remote_path).await
    }

    /// Downloads a cloud file to a local destination path.
    pub async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.backend.download(remote_path, local_path).await
    }

    /// Deletes a file or folder at the specified remote path.
    pub async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.backend.delete(remote_path).await
    }

    /// Lists metadata of files and folders located under `remote_path`.
    pub async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.backend.list(remote_path).await
    }

    /// Creates a directory hierarchy remotely if supported.
    pub async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.backend.create_folder(remote_path).await
    }

    /// Moves or renames a remote file.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if let Some(ops) = self.backend.as_server_side_ops() {
            ops.rename(from, to).await
        } else {
            self.backend.rename(from, to).await
        }
    }

    /// Copies a remote file to a new location.
    pub async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if let Some(ops) = self.backend.as_server_side_ops() {
            ops.copy(from, to).await
        } else {
            self.backend.copy(from, to).await
        }
    }

    /// Verifies bidirectional integrity between `local_root` and `remote_path` (`rclone check` equivalent).
    pub async fn verify_integrity(
        &self,
        local_root: &Path,
        remote_path: &str,
        compare_checksums: bool,
    ) -> Result<crate::verifier::VerificationReport, StorageError> {
        crate::verifier::verify_integrity(
            self.backend.as_ref(),
            local_root,
            remote_path,
            compare_checksums,
        )
        .await
    }

    /// Scans `remote_path` for duplicate files (`rclone dedupe` equivalent).
    pub async fn scan_duplicates(
        &self,
        remote_path: &str,
    ) -> Result<Vec<crate::dedupe::DuplicateGroup>, StorageError> {
        crate::dedupe::scan_duplicates(self.backend.as_ref(), remote_path).await
    }

    /// Resolves identified duplicate file groups using `strategy`.
    pub async fn resolve_duplicates(
        &self,
        groups: &[crate::dedupe::DuplicateGroup],
        strategy: crate::dedupe::DedupeStrategy,
    ) -> Result<crate::dedupe::DedupeSummary, StorageError> {
        crate::dedupe::resolve_duplicates(self.backend.as_ref(), groups, strategy).await
    }

    /// Computes storage statistics and extension breakdown (`rclone size` equivalent).
    pub async fn compute_analytics(
        &self,
        remote_path: &str,
    ) -> Result<crate::analytics::StorageAnalytics, StorageError> {
        crate::analytics::compute_storage_analytics(self.backend.as_ref(), remote_path).await
    }

    /// Builds a hierarchical directory tree up to `max_depth` (`rclone tree` equivalent).
    pub async fn build_tree(
        &self,
        remote_path: &str,
        max_depth: Option<usize>,
    ) -> Result<crate::analytics::TreeNode, StorageError> {
        crate::analytics::build_remote_tree(self.backend.as_ref(), remote_path, max_depth).await
    }

    /// Transfers or synchronizes files directly from this client to `dest` (`rclone copy`/`sync` equivalent).
    pub async fn transfer_to(
        &self,
        dest: &CloudSyncClient,
        source_prefix: &str,
        dest_prefix: &str,
        options: crate::cloud_transfer::CloudTransferOptions,
    ) -> Result<crate::cloud_transfer::CloudTransferReport, StorageError> {
        crate::cloud_transfer::transfer_between_backends(
            self.backend.as_ref(),
            dest.backend.as_ref(),
            source_prefix,
            dest_prefix,
            options,
        )
        .await
    }

    /// Creates a content-addressable deduplicated point-in-time snapshot of `local_root` (Kopia/Restic equivalent).
    pub async fn create_snapshot(
        &self,
        local_root: &Path,
        label: &str,
    ) -> Result<crate::snapshot::SnapshotManifest, StorageError> {
        crate::snapshot::create_snapshot(self.backend.as_ref(), local_root, label).await
    }

    /// Lists all point-in-time snapshots stored on this backend.
    pub async fn list_snapshots(
        &self,
    ) -> Result<Vec<crate::snapshot::SnapshotManifest>, StorageError> {
        crate::snapshot::list_snapshots(self.backend.as_ref()).await
    }

    /// Restores a point-in-time snapshot into `target_dir` with SHA-256 verification.
    pub async fn restore_snapshot(
        &self,
        snapshot_id: &str,
        target_dir: &Path,
    ) -> Result<crate::snapshot::SnapshotRestoreReport, StorageError> {
        crate::snapshot::restore_snapshot(self.backend.as_ref(), snapshot_id, target_dir).await
    }

    /// Prunes old snapshots keeping the newest `keep_last_n` and garbage-collects unreferenced content blobs.
    pub async fn prune_snapshots(
        &self,
        keep_last_n: usize,
    ) -> Result<crate::snapshot::SnapshotPruneReport, StorageError> {
        crate::snapshot::prune_snapshots(self.backend.as_ref(), keep_last_n).await
    }
}

/// Fluent builder for constructing a configured `CloudSyncClient`.
#[derive(Default)]
pub struct CloudSyncBuilder {
    backend: Option<Arc<dyn StorageBackend>>,
    provider_name: Option<String>,
    provider_config: Option<JsonValue>,
    sim_root: Option<PathBuf>,
    registry: Option<Arc<DynamicBackendRegistry>>,
    retry: Option<RetryConfig>,
    circuit_breaker: Option<CircuitBreakerConfig>,
    rate_limits: Option<(u64, u64)>,
    enable_logging: bool,
    enable_metrics: bool,
    enable_compression: bool,
    encryption_password: Option<String>,
    cache_ttl: Option<Duration>,
    chunk_size_bytes: Option<usize>,
    http_transport: Option<Arc<dyn HttpTransport>>,
}

impl CloudSyncBuilder {
    /// Creates a default empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an already constructed backend.
    pub fn with_backend(mut self, backend: Arc<dyn StorageBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Configures a multi-cloud virtual union backend combining `upstreams` under `policy`.
    pub fn with_union(
        mut self,
        name: impl Into<String>,
        upstreams: Vec<Arc<dyn StorageBackend>>,
        policy: UnionPolicy,
    ) -> Self {
        self.backend = Some(Arc::new(UnionBackend::new(name, upstreams, policy)));
        self
    }

    /// Configures a local directory simulation fallback backend.
    pub fn with_local_sim(
        mut self,
        sim_root: impl Into<PathBuf>,
        provider_name: impl Into<String>,
    ) -> Self {
        let root = sim_root.into();
        let name = provider_name.into();
        let backend = Arc::new(crate::providers::local_sim::LocalSimulation::new(
            root, name,
        ));
        self.backend = Some(backend);
        self
    }

    /// Configures a backend dynamically via name and JSON configuration.
    pub fn with_provider(
        mut self,
        provider_name: impl Into<String>,
        config: JsonValue,
        sim_root: impl Into<PathBuf>,
    ) -> Self {
        self.provider_name = Some(provider_name.into());
        self.provider_config = Some(config);
        self.sim_root = Some(sim_root.into());
        self
    }

    /// Sets the dynamic backend registry to use for lookup (defaults to global registry).
    pub fn with_registry(mut self, registry: Arc<DynamicBackendRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Configures automatic retry middleware with exponential backoff.
    pub fn with_retry(
        mut self,
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Self {
        self.retry = Some(RetryConfig {
            max_attempts,
            base_delay,
            max_delay,
        });
        self
    }

    /// Configures circuit breaker middleware for fast-failing during sustained outages.
    pub fn with_circuit_breaker(
        mut self,
        failure_threshold: u32,
        recovery_timeout: Duration,
    ) -> Self {
        self.circuit_breaker = Some(CircuitBreakerConfig {
            failure_threshold,
            recovery_timeout,
            probe_success_threshold: 2,
        });
        self
    }

    /// Configures bandwidth rate limiting (upload_bytes_per_sec, download_bytes_per_sec).
    pub fn with_rate_limits(mut self, upload_bps: u64, download_bps: u64) -> Self {
        self.rate_limits = Some((upload_bps, download_bps));
        self
    }

    /// Enables structured tracing / logging middleware.
    pub fn with_logging(mut self) -> Self {
        self.enable_logging = true;
        self
    }

    /// Enables metrics collection middleware.
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Enables transparent lossless payload compression (`CLZ1`).
    pub fn with_compression(mut self) -> Self {
        self.enable_compression = true;
        self
    }

    /// Enables client-side zero-knowledge AES-256-GCM encryption with `password`.
    pub fn with_encryption(mut self, password: impl Into<String>) -> Self {
        self.encryption_password = Some(password.into());
        self
    }

    /// Enables TTL metadata caching for directory listings with write-through invalidation.
    pub fn with_cache(mut self, ttl: Duration) -> Self {
        self.cache_ttl = Some(ttl);
        self
    }

    /// Enables transparent large-file chunking for files exceeding `chunk_size_bytes`.
    pub fn with_chunker(mut self, chunk_size_bytes: usize) -> Self {
        self.chunk_size_bytes = Some(chunk_size_bytes);
        self
    }

    /// Injects a custom HTTP transport (useful for mocks or offline unit testing).
    pub fn with_http_transport(mut self, transport: Arc<dyn HttpTransport>) -> Self {
        self.http_transport = Some(transport);
        self
    }

    /// Finalizes construction and returns a layered `CloudSyncClient`.
    pub fn build(self) -> Result<CloudSyncClient, StorageError> {
        let mut backend = if let Some(b) = self.backend {
            b
        } else if let (Some(name), Some(cfg), Some(root)) =
            (self.provider_name, self.provider_config, self.sim_root)
        {
            let registry = self
                .registry
                .unwrap_or_else(|| Arc::new(DynamicBackendRegistry::with_defaults()));
            registry.create_backend(&name, &cfg, root)?
        } else {
            return Err(StorageError::Provider {
                message: "No backend or provider configuration provided to CloudSyncBuilder".into(),
                status: None,
            });
        };

        // Apply layers in standard order: CircuitBreaker -> Retry -> RateLimit -> Metrics -> Logging
        if let Some(cb_cfg) = self.circuit_breaker {
            backend = backend.with_layer(CircuitBreakerLayer::new(cb_cfg));
        }

        if let Some(retry_cfg) = self.retry {
            backend = backend.with_layer(RetryLayer::new(retry_cfg));
        }

        if let Some((up, down)) = self.rate_limits {
            let up_bucket = if up > 0 {
                Some(TokenBucket::new(up))
            } else {
                None
            };
            let down_bucket = if down > 0 {
                Some(TokenBucket::new(down))
            } else {
                None
            };
            backend = backend.with_layer(RateLimitLayer::new(up_bucket, down_bucket));
        }

        if self.enable_metrics {
            backend = backend.with_layer(MetricsLayer::default());
        }

        if self.enable_compression {
            backend = backend.with_layer(CompressionLayer::new());
        }

        if let Some(pwd) = self.encryption_password {
            backend = Arc::new(EncryptedBackend::new(backend, &pwd));
        }

        if let Some(chunk_size) = self.chunk_size_bytes {
            backend = Arc::new(ChunkerBackend::new(backend, chunk_size));
        }

        if let Some(ttl) = self.cache_ttl {
            backend = backend.with_layer(CacheLayer::new(ttl));
        }

        if self.enable_logging {
            backend = backend.with_layer(LoggingLayer);
        }

        Ok(CloudSyncClient::new(backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_client_builder_local_sim() {
        let dir = tempdir().unwrap();
        let client = CloudSyncClient::builder()
            .with_local_sim(dir.path(), "test_local")
            .with_retry(2, Duration::from_millis(50), Duration::from_millis(200))
            .with_logging()
            .with_metrics()
            .build()
            .expect("Builder failed");

        assert_eq!(client.provider_name(), "test_local");

        // Write a test file
        let local_file = dir.path().join("source.txt");
        tokio::fs::write(&local_file, b"Hello CloudSync")
            .await
            .unwrap();

        // Test upload through client facade
        client
            .upload(&local_file, "destination.txt")
            .await
            .expect("Upload failed");

        // Test list through client facade
        let items = client.list("").await.expect("List failed");
        assert!(items
            .iter()
            .any(|i| i.path.to_str().unwrap().contains("destination.txt")));

        // Test download through client facade
        let downloaded_file = dir.path().join("downloaded.txt");
        client
            .download("destination.txt", &downloaded_file)
            .await
            .expect("Download failed");
        let content = tokio::fs::read_to_string(&downloaded_file).await.unwrap();
        assert_eq!(content, "Hello CloudSync");
    }
}
