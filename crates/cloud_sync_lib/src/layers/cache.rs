//! Metadata & Directory Listing Cache Middleware Layer (Rclone VFS cache equivalent).
//!
//! Caches `list()` results in memory with a configurable time-to-live (`ttl`) and
//! performs write-through cache invalidation whenever files or folders are mutated
//! (`upload`, `delete`, `create_folder`, `rename`, `copy`).

use crate::layers::StorageLayer;
use crate::traits::{StorageBackend, StorageCapabilities, StorageError, StorageItem};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Composable middleware layer adding TTL directory listing caching to any [`StorageBackend`].
#[derive(Debug, Clone)]
pub struct CacheLayer {
    pub ttl: Duration,
}

impl CacheLayer {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl }
    }
}

impl Default for CacheLayer {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
        }
    }
}

impl StorageLayer for CacheLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(CachedBackend::new(backend, self.ttl))
    }
}

struct CacheEntry {
    items: Vec<StorageItem>,
    inserted_at: Instant,
}

/// Storage backend wrapper providing TTL directory listing cache and automatic write-through invalidation.
pub struct CachedBackend {
    inner: Arc<dyn StorageBackend>,
    ttl: Duration,
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl CachedBackend {
    pub fn new(inner: Arc<dyn StorageBackend>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    fn normalize_key(remote_path: &str) -> String {
        remote_path.trim_matches('/').replace('\\', "/")
    }

    /// Clears all cached directory listings.
    pub fn invalidate_all(&self) {
        if let Ok(mut lock) = self.entries.write() {
            lock.clear();
        }
    }

    /// Invalidates the given path and its parent directory in the cache.
    pub fn invalidate_path(&self, remote_path: &str) {
        let norm = Self::normalize_key(remote_path);
        if let Ok(mut lock) = self.entries.write() {
            lock.remove(&norm);
            if let Some(pos) = norm.rfind('/') {
                lock.remove(&norm[..pos]);
            } else {
                lock.remove("");
            }
        }
    }

    /// Returns the current number of cached directory paths.
    pub fn cached_dir_count(&self) -> usize {
        self.entries.read().map(|l| l.len()).unwrap_or(0)
    }
}

#[async_trait]
impl StorageBackend for CachedBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let key = Self::normalize_key(remote_path);
        if let Ok(lock) = self.entries.read() {
            if let Some(entry) = lock.get(&key) {
                if entry.inserted_at.elapsed() < self.ttl {
                    return Ok(entry.items.clone());
                }
            }
        }

        let items = self.inner.list(remote_path).await?;
        if let Ok(mut lock) = self.entries.write() {
            lock.insert(
                key,
                CacheEntry {
                    items: items.clone(),
                    inserted_at: Instant::now(),
                },
            );
        }
        Ok(items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let res = self.inner.upload(local_path, remote_path).await;
        if res.is_ok() {
            self.invalidate_path(remote_path);
        }
        res
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.inner.download(remote_path, local_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let res = self.inner.delete(remote_path).await;
        if res.is_ok() {
            self.invalidate_path(remote_path);
        }
        res
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let res = self.inner.create_folder(remote_path).await;
        if res.is_ok() {
            self.invalidate_path(remote_path);
        }
        res
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let res = self.inner.rename(from, to).await;
        if res.is_ok() {
            self.invalidate_path(from);
            self.invalidate_path(to);
        }
        res
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let res = self.inner.copy(from, to).await;
        if res.is_ok() {
            self.invalidate_path(to);
        }
        res
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cached_backend_ttl_and_write_invalidation() {
        let remote_dir = tempdir().unwrap();
        let local_dir = tempdir().unwrap();

        let sim = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "CachedSim".into(),
        ));
        let cached = CachedBackend::new(sim.clone(), Duration::from_secs(60));

        let f1 = local_dir.path().join("f1.txt");
        tokio::fs::write(&f1, b"hello").await.unwrap();
        cached.upload(&f1, "f1.txt").await.unwrap();

        // Populate cache
        let list1 = cached.list("").await.unwrap();
        assert_eq!(list1.len(), 1);
        assert_eq!(cached.cached_dir_count(), 1);

        // Mutate through CachedBackend -> invalidates cache automatically
        cached.upload(&f1, "f2.txt").await.unwrap();
        assert_eq!(cached.cached_dir_count(), 0);

        let list2 = cached.list("").await.unwrap();
        assert_eq!(list2.len(), 2);
    }
}
