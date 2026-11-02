//! Telemetry and metrics layer tracking operation counts, bytes transferred, and errors.

use super::StorageLayer;
use cloud_sync_core::{
    ChangeFeed, RemoteChange, ResumableUploadSession, ResumableUploader, ServerSideOperations,
    StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct StorageMetrics {
    pub upload_bytes: AtomicU64,
    pub download_bytes: AtomicU64,
    pub upload_count: AtomicU64,
    pub download_count: AtomicU64,
    pub delete_count: AtomicU64,
    pub list_count: AtomicU64,
    pub error_count: AtomicU64,
}

#[derive(Clone, Default)]
pub struct MetricsCollector {
    metrics: Arc<StorageMetrics>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(StorageMetrics::default()),
        }
    }

    pub fn metrics(&self) -> &StorageMetrics {
        &self.metrics
    }

    pub fn total_upload_bytes(&self) -> u64 {
        self.metrics.upload_bytes.load(Ordering::Relaxed)
    }

    pub fn total_download_bytes(&self) -> u64 {
        self.metrics.download_bytes.load(Ordering::Relaxed)
    }
}

pub struct MetricsLayer {
    collector: MetricsCollector,
}

impl MetricsLayer {
    pub fn new(collector: MetricsCollector) -> Self {
        Self { collector }
    }

    pub fn collector(&self) -> &MetricsCollector {
        &self.collector
    }
}

impl Default for MetricsLayer {
    fn default() -> Self {
        Self::new(MetricsCollector::new())
    }
}

impl StorageLayer for MetricsLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(MetricsBackend {
            inner: backend,
            collector: self.collector.clone(),
        })
    }
}

pub struct MetricsBackend {
    inner: Arc<dyn StorageBackend>,
    collector: MetricsCollector,
}

#[async_trait::async_trait]
impl StorageBackend for MetricsBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }

    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        self.inner.as_server_side_ops()
    }

    fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
        self.inner.as_change_feed()
    }

    fn as_resumable_uploader(&self) -> Option<&dyn ResumableUploader> {
        self.inner.as_resumable_uploader()
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let size = tokio::fs::metadata(local_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let start = Instant::now();
        let res = self.inner.upload(local_path, remote_path).await;
        if res.is_ok() {
            self.collector
                .metrics
                .upload_count
                .fetch_add(1, Ordering::Relaxed);
            self.collector
                .metrics
                .upload_bytes
                .fetch_add(size, Ordering::Relaxed);
        } else {
            self.collector
                .metrics
                .error_count
                .fetch_add(1, Ordering::Relaxed);
        }
        let _ = start.elapsed();
        res
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let res = self.inner.download(remote_path, local_path).await;
        if res.is_ok() {
            let size = tokio::fs::metadata(local_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            self.collector
                .metrics
                .download_count
                .fetch_add(1, Ordering::Relaxed);
            self.collector
                .metrics
                .download_bytes
                .fetch_add(size, Ordering::Relaxed);
        } else {
            self.collector
                .metrics
                .error_count
                .fetch_add(1, Ordering::Relaxed);
        }
        res
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let res = self.inner.delete(remote_path).await;
        if res.is_ok() {
            self.collector
                .metrics
                .delete_count
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.collector
                .metrics
                .error_count
                .fetch_add(1, Ordering::Relaxed);
        }
        res
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let res = self.inner.list(remote_path).await;
        if res.is_ok() {
            self.collector
                .metrics
                .list_count
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.collector
                .metrics
                .error_count
                .fetch_add(1, Ordering::Relaxed);
        }
        res
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.copy(from, to).await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        self.inner
            .start_resumable_upload(local_path, remote_path, chunk_size)
            .await
    }

    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        self.inner.compute_local_checksum(local_path).await
    }

    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        self.inner.get_latest_cursor().await
    }

    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        self.inner.fetch_changes(cursor).await
    }

    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }

    fn simulation_reason(&self) -> Option<String> {
        self.inner.simulation_reason()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_metrics_layer_tracking() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_storage");
        tokio::fs::create_dir_all(&sim_root).await.unwrap();

        let local_sim: Arc<dyn StorageBackend> =
            Arc::new(LocalSimulation::new(sim_root, "local_sim".to_string()));
        let collector = MetricsCollector::new();
        let layer = MetricsLayer::new(collector.clone());
        let wrapped = layer.layer(local_sim);

        assert_eq!(wrapped.name(), "local_sim");
        assert_eq!(collector.total_upload_bytes(), 0);
        assert_eq!(collector.total_download_bytes(), 0);

        // Upload
        let file_path = dir.path().join("payload.txt");
        let payload = b"Hello telemetry tracking!";
        tokio::fs::write(&file_path, payload).await.unwrap();

        wrapped
            .upload(&file_path, "telemetry/file.txt")
            .await
            .expect("upload should succeed");
        assert_eq!(collector.total_upload_bytes(), payload.len() as u64);
        assert_eq!(collector.metrics().upload_count.load(Ordering::Relaxed), 1);

        // List
        let items = wrapped
            .list("telemetry")
            .await
            .expect("list should succeed");
        assert_eq!(items.len(), 1);
        assert_eq!(collector.metrics().list_count.load(Ordering::Relaxed), 1);

        // Download
        let dl_path = dir.path().join("downloaded.txt");
        wrapped
            .download("telemetry/file.txt", &dl_path)
            .await
            .expect("download should succeed");
        assert_eq!(collector.total_download_bytes(), payload.len() as u64);
        assert_eq!(
            collector.metrics().download_count.load(Ordering::Relaxed),
            1
        );

        // Delete
        wrapped
            .delete("telemetry/file.txt")
            .await
            .expect("delete should succeed");
        assert_eq!(collector.metrics().delete_count.load(Ordering::Relaxed), 1);

        // Error tracking on missing download
        let missing_res = wrapped.download("telemetry/missing.bin", &dl_path).await;
        assert!(missing_res.is_err());
        assert_eq!(collector.metrics().error_count.load(Ordering::Relaxed), 1);
    }
}
