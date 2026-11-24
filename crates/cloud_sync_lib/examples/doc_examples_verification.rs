//! # Documentation Examples Verification
//!
//! This example program verifies that all API code snippets documented in
//! `docs/CLIENT_API.md`, `docs/EXTENDING_AND_TESTING.md`, and crate READMEs
//! compile and execute correctly against the current library architecture.
//!
//! Run with:
//! ```bash
//! cargo run --example doc_examples_verification -p cloud_sync_lib
//! ```

use async_trait::async_trait;
use cloud_sync_core::{
    DirectoryLister, FolderOps, NormalizedPath, StorageBackend, StorageCapabilities, StorageError,
    StorageItem, StorageReader, StorageWriter,
};
use cloud_sync_lib::layers::{
    CacheLayer, CircuitBreakerConfig, CircuitBreakerLayer, CompressionLayer, LayeredBackendExt,
    RetryConfig, RetryLayer, StorageLayer,
};
use cloud_sync_lib::providers::chunker::ChunkerBackend;
use cloud_sync_lib::providers::union::{UnionBackend, UnionPolicy};
use cloud_sync_lib::snapshot::list_snapshots;
use cloud_sync_lib::cloud_transfer::{transfer_between_backends, CloudTransferOptions};
use cloud_sync_lib::analytics::compute_storage_analytics;
use cloud_sync_lib::{
    BackendFactory, BufferPool, CloudSyncClient, DynamicBackendRegistry,
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

// --- Snippet 1: MemoryStorageBackend from EXTENDING_AND_TESTING.md ---
pub struct MemoryStorageBackend {
    files: RwLock<HashMap<String, Vec<u8>>>,
}

impl MemoryStorageBackend {
    pub fn new() -> Self {
        Self {
            files: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageReader for MemoryStorageBackend {
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let norm = NormalizedPath::new(remote_path)
            .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
        let maybe_bytes = {
            let files = self.files.read().map_err(|_| StorageError::Provider {
                message: "Lock poisoned".into(),
                status: None,
            })?;
            files.get(norm.as_str()).cloned()
        };
        if let Some(bytes) = maybe_bytes {
            tokio::fs::write(local_path, bytes).await.map_err(StorageError::Io)?;
            Ok(())
        } else {
            Err(StorageError::NotFound(remote_path.to_string()))
        }
    }
}

#[async_trait]
impl StorageWriter for MemoryStorageBackend {
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let norm = NormalizedPath::new(remote_path)
            .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
        let bytes = tokio::fs::read(local_path).await.map_err(StorageError::Io)?;
        let mut files = self.files.write().map_err(|_| StorageError::Provider {
            message: "Lock poisoned".into(),
            status: None,
        })?;
        files.insert(norm.as_str().to_string(), bytes);
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let norm = NormalizedPath::new(remote_path)
            .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
        let mut files = self.files.write().map_err(|_| StorageError::Provider {
            message: "Lock poisoned".into(),
            status: None,
        })?;
        files.remove(norm.as_str());
        Ok(())
    }
}

#[async_trait]
impl DirectoryLister for MemoryStorageBackend {
    async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let files = self.files.read().map_err(|_| StorageError::Provider {
            message: "Lock poisoned".into(),
            status: None,
        })?;
        let items = files
            .iter()
            .map(|(path, data)| StorageItem {
                path: std::path::PathBuf::from(path),
                size: data.len() as u64,
                modified: std::time::SystemTime::now(),
                is_dir: false,
                checksum: None,
                permissions: None,
            })
            .collect();
        Ok(items)
    }
}

#[async_trait]
impl FolderOps for MemoryStorageBackend {
    async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn rename(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for MemoryStorageBackend {
    fn name(&self) -> &str {
        "in_memory"
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::NONE
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        StorageWriter::upload(self, local_path, remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        StorageReader::download(self, remote_path, local_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        StorageWriter::delete(self, remote_path).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        DirectoryLister::list(self, remote_path).await
    }
}

// --- Snippet 2: MemoryFactory from EXTENDING_AND_TESTING.md ---
struct MemoryFactory;

impl BackendFactory for MemoryFactory {
    fn provider_name(&self) -> &'static str {
        "memory"
    }

    fn create(
        &self,
        _config: &JsonValue,
        _sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        Ok(Arc::new(MemoryStorageBackend::new()))
    }
}

// --- Snippet 3: Custom Layer from EXTENDING_AND_TESTING.md ---
pub struct AuditLayer {
    pub audit_tag: String,
}

impl StorageLayer for AuditLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(AuditBackend {
            inner: backend,
            tag: self.audit_tag.clone(),
        })
    }
}

pub struct AuditBackend {
    inner: Arc<dyn StorageBackend>,
    tag: String,
}

#[async_trait]
impl StorageReader for AuditBackend {
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.inner.download(remote_path, local_path).await
    }
}

#[async_trait]
impl StorageWriter for AuditBackend {
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        println!("[AUDIT {}] Uploading {}", self.tag, remote_path);
        self.inner.upload(local_path, remote_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.delete(remote_path).await
    }
}

#[async_trait]
impl DirectoryLister for AuditBackend {
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.inner.list(remote_path).await
    }
}

#[async_trait]
impl FolderOps for AuditBackend {
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to).await
    }
}

#[async_trait]
impl StorageBackend for AuditBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        self.inner.upload(local_path, remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.inner.download(remote_path, local_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.delete(remote_path).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.inner.list(remote_path).await
    }
}

// --- Snippet 4: Layer wrapping function from CLIENT_API.md ---
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

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    println!("=== Testing Document Example Code Snippets ===");

    // 1. Verify Basic Setup with Local Simulation
    let client = CloudSyncClient::builder()
        .with_local_sim("cloud_simulation", "local_backup")
        .with_retry(3, Duration::from_millis(100), Duration::from_secs(2))
        .with_logging()
        .with_metrics()
        .build()?;
    assert_eq!(client.provider_name(), "local_backup");

    // 2. Verify BufferPool snippet
    let pool = BufferPool::new(64 * 1024, 16);
    let mut buffer = pool.acquire();
    buffer.extend_from_slice(b"SYNC");
    assert_eq!(&buffer[..4], b"SYNC");
    drop(buffer);

    // 3. Verify Dynamic Registry registration & custom provider
    let registry = Arc::new(DynamicBackendRegistry::new());
    registry.register(Box::new(MemoryFactory));
    let custom_backend = registry.create_backend("memory", &serde_json::json!({}), PathBuf::from("/tmp"))?;
    assert_eq!(custom_backend.name(), "in_memory");

    // 4. Verify Layer Wrapping
    let layered = wrap_backend(custom_backend);
    assert_eq!(layered.name(), "in_memory");

    // 5. Verify AuditLayer custom layer
    let audited = AuditLayer {
        audit_tag: "DOC_TEST".to_string(),
    }
    .layer(layered.clone());
    assert_eq!(audited.name(), "in_memory");

    // 6. Verify UnionBackend snippet
    let b1 = Arc::new(MemoryStorageBackend::new());
    let b2 = Arc::new(MemoryStorageBackend::new());
    let union_backend = Arc::new(UnionBackend::new("UnionRAID1", vec![b1.clone(), b2.clone()], UnionPolicy::MirrorAll));
    assert_eq!(union_backend.name(), "UnionRAID1");

    // 7. Verify ChunkerBackend snippet
    let chunker = Arc::new(ChunkerBackend::new(b1.clone(), 1024 * 1024));
    assert_eq!(chunker.name(), "in_memory");

    // 8. Verify CompressionLayer & CacheLayer
    let compressed_and_cached = b1
        .clone()
        .with_layer(CompressionLayer::new())
        .with_layer(CacheLayer::new(Duration::from_secs(10)));
    assert_eq!(compressed_and_cached.name(), "in_memory");

    // 9. Verify transfer_between_backends snippet
    let transfer_options = CloudTransferOptions {
        delete_extraneous: false,
        skip_matching: true,
        dry_run: true,
    };
    let transfer_report = transfer_between_backends(
        b1.as_ref(),
        b2.as_ref(),
        "source",
        "dest",
        transfer_options,
    ).await?;
    assert_eq!(transfer_report.source_backend, "in_memory");

    // 10. Verify snapshot functions
    let snapshots = list_snapshots(b1.as_ref()).await?;
    assert!(snapshots.is_empty());

    // 11. Verify compute_storage_analytics snippet
    let analytics = compute_storage_analytics(b1.as_ref(), "").await?;
    assert_eq!(analytics.total_files, 0);

    println!("All documentation code examples compiled and verified successfully!");
    Ok(())
}
