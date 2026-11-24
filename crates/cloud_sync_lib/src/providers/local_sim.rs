//! Local folder fallback simulator.
//!
//! Provides an implementation of storage simulation on the local filesystem.

use cloud_sync_core::NormalizedPath;
use crate::rate_limit::{copy_rate_limited, TokenBucket};
use crate::traits::{StorageBackend, StorageError, StorageItem};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tokio::fs;
use tracing::info;

const MAX_CHECKSUM_CACHE_ENTRIES: usize = 4096;
type ChecksumCacheMap = HashMap<PathBuf, (u64, SystemTime, String)>;

/// Local folder fallback simulator.
///
/// Implements mock filesystem storage behavior for offline development and testing
/// when cloud credentials are not configured.
pub struct LocalSimulation {
    /// The root directory representing the simulated remote storage.
    root_dir: PathBuf,
    /// The name of the provider we are simulating (e.g. "Dropbox").
    provider_name: String,
    /// Optional upload rate limiter
    upload_limiter: Option<TokenBucket>,
    /// Optional download rate limiter
    download_limiter: Option<TokenBucket>,
    /// Metadata-keyed `(size, mtime) -> sha256` cache to avoid re-reading unchanged files on `list()`.
    checksum_cache: Arc<RwLock<ChecksumCacheMap>>,
}

impl LocalSimulation {
    /// Creates a new `LocalSimulation` instance with a given root directory and provider name.
    ///
    /// # Arguments
    /// * `root_dir` - The root path to use for simulation.
    /// * `provider_name` - The provider name to mock.
    ///
    /// # Returns
    /// A new instance of `LocalSimulation`.
    pub fn new(root_dir: PathBuf, provider_name: String) -> Self {
        let _ = std::fs::create_dir_all(&root_dir);
        Self {
            root_dir,
            provider_name,
            upload_limiter: None,
            download_limiter: None,
            checksum_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Looks up or computes the SHA-256 checksum of `path` keyed by `(size, modified)`.
    async fn get_or_compute_checksum(
        &self,
        path: &Path,
        size: u64,
        modified: SystemTime,
    ) -> Option<String> {
        if let Ok(cache) = self.checksum_cache.read() {
            if let Some((cached_size, cached_mtime, cached_hash)) = cache.get(path) {
                if *cached_size == size && *cached_mtime == modified {
                    return Some(cached_hash.clone());
                }
            }
        }

        let hash = crate::checksum::compute_sha256(path).await.ok()?;
        if let Ok(mut cache) = self.checksum_cache.write() {
            if cache.len() >= MAX_CHECKSUM_CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(path.to_path_buf(), (size, modified, hash.clone()));
        }
        Some(hash)
    }

    /// Invalidates any cached checksum entry for `path`.
    fn invalidate_cached_checksum(&self, path: &Path) {
        if let Ok(mut cache) = self.checksum_cache.write() {
            cache.remove(path);
        }
    }

    /// Sets the upload and download rate limiters.
    pub fn with_limiters(
        mut self,
        upload_limiter: Option<TokenBucket>,
        download_limiter: Option<TokenBucket>,
    ) -> Self {
        self.upload_limiter = upload_limiter;
        self.download_limiter = download_limiter;
        self
    }

    /// Safely resolves a remote path against `root_dir`, verifying against path traversal
    /// attacks and ensuring the target resides strictly within the simulation root.
    pub fn try_resolve(&self, remote_path: &str) -> Result<PathBuf, StorageError> {
        let norm = NormalizedPath::new(remote_path).map_err(|e| {
            StorageError::InvalidPath(format!("Path traversal or illegal name rejected in {}: {}", self.provider_name, e))
        })?;
        let resolved = if norm.is_empty() {
            self.root_dir.clone()
        } else {
            self.root_dir.join(norm.as_str())
        };
        Ok(resolved)
    }

    /// Maps a remote path to the local directory simulation structure with sanitization fallback.
    pub fn resolve(&self, remote_path: &str) -> PathBuf {
        self.try_resolve(remote_path).unwrap_or_else(|_| {
            let sanitized = NormalizedPath::empty().join(remote_path);
            if sanitized.is_empty() {
                self.root_dir.clone()
            } else {
                self.root_dir.join(sanitized.as_str())
            }
        })
    }

    /// Simulates uploading a file by copying it to the local simulation folder.
    pub async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let destination = self.try_resolve(remote_path)?;
        info!(
            "[{}] (Simulated) Uploading local file {:?} to remote path '{}'",
            self.provider_name, local_path, remote_path
        );
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        copy_rate_limited(local_path, &destination, self.upload_limiter.clone()).await?;
        self.invalidate_cached_checksum(&destination);
        if let Ok(metadata) = std::fs::metadata(local_path) {
            if let Ok(modified) = metadata.modified() {
                let ft = filetime::FileTime::from_system_time(modified);
                let _ = filetime::set_file_mtime(&destination, ft);
            }
            let _ = std::fs::set_permissions(&destination, metadata.permissions());
        }
        Ok(())
    }

    /// Simulates downloading a file by copying it from the local simulation folder.
    pub async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let source = self.try_resolve(remote_path)?;
        info!(
            "[{}] (Simulated) Downloading remote path '{}' to local file {:?}",
            self.provider_name, remote_path, local_path
        );
        if !source.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        copy_rate_limited(&source, local_path, self.download_limiter.clone()).await?;
        if let Ok(metadata) = std::fs::metadata(&source) {
            let _ = std::fs::set_permissions(local_path, metadata.permissions());
        }
        Ok(())
    }

    /// Simulates deleting a file or directory from the local simulation folder.
    pub async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.try_resolve(remote_path)?;
        info!(
            "[{}] (Simulated) Deleting remote path '{}'",
            self.provider_name, remote_path
        );
        if !target.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }
        if target.is_dir() {
            fs::remove_dir_all(&target).await?;
        } else {
            fs::remove_file(&target).await?;
        }
        self.invalidate_cached_checksum(&target);
        Ok(())
    }

    /// Simulates creating a folder/directory in the local simulation folder.
    pub async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.try_resolve(remote_path)?;
        info!(
            "[{}] (Simulated) Creating remote directory '{}'",
            self.provider_name, remote_path
        );
        fs::create_dir_all(&target).await?;
        Ok(())
    }

    /// Simulates copying a file or folder in the local simulation folder.
    pub async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let source = self.try_resolve(from)?;
        let dest = self.try_resolve(to)?;
        info!(
            "[{}] (Simulated) Copying remote path '{}' to '{}'",
            self.provider_name, from, to
        );
        if !source.exists() {
            return Err(StorageError::NotFound(from.to_string()));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&source, &dest).await?;
        self.invalidate_cached_checksum(&dest);
        Ok(())
    }

    /// Simulates renaming a file or folder in the local simulation folder.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let source = self.try_resolve(from)?;
        let dest = self.try_resolve(to)?;
        info!(
            "[{}] (Simulated) Renaming remote path '{}' to '{}'",
            self.provider_name, from, to
        );
        if !source.exists() {
            return Err(StorageError::NotFound(from.to_string()));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(&source, &dest).await?;
        self.invalidate_cached_checksum(&source);
        self.invalidate_cached_checksum(&dest);
        Ok(())
    }

    /// Simulates listing contents of the local simulation folder.
    pub async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target = self.try_resolve(remote_path)?;
        info!(
            "[{}] (Simulated) Listing contents of remote path '{}'",
            self.provider_name, remote_path
        );
        if !target.exists() {
            if remote_path.trim_matches('/').is_empty() {
                let _ = fs::create_dir_all(&target).await;
            } else {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
        }
        let mut items = Vec::new();
        let mut entries = fs::read_dir(&target).await?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let is_dir = metadata.is_dir();
            let size = metadata.len();
            let modified = metadata.modified().unwrap_or_else(|_| SystemTime::now());
            let entry_path = entry.path();
            let checksum = if is_dir {
                None
            } else {
                self.get_or_compute_checksum(&entry_path, size, modified)
                    .await
            };
            items.push(StorageItem {
                path: entry_path
                    .strip_prefix(&self.root_dir)
                    .unwrap_or(&entry_path)
                    .to_path_buf(),
                size,
                modified,
                is_dir,
                checksum,
                permissions: cloud_sync_core::get_permissions(&metadata.permissions()),
            });
        }
        Ok(items)
    }
}

pub struct SimulatedResumableUploadSession {
    local_path: std::path::PathBuf,
    dest_path: std::path::PathBuf,
    total_bytes: u64,
    uploaded_bytes: u64,
    chunk_size: usize,
}

#[async_trait]
impl cloud_sync_core::ResumableUploadSession for SimulatedResumableUploadSession {
    fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    fn uploaded_bytes(&self) -> u64 {
        self.uploaded_bytes
    }
    async fn upload_next_chunk(&mut self) -> Result<(u64, u64), StorageError> {
        if self.is_complete() {
            return Ok((self.uploaded_bytes, self.total_bytes));
        }
        let remaining = self.total_bytes - self.uploaded_bytes;
        let to_upload = (self.chunk_size as u64).min(remaining);
        self.uploaded_bytes += to_upload;
        Ok((self.uploaded_bytes, self.total_bytes))
    }
    fn is_complete(&self) -> bool {
        self.uploaded_bytes >= self.total_bytes
    }
    async fn finish(&mut self) -> Result<Option<String>, StorageError> {
        if let Some(parent) = self.dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&self.local_path, &self.dest_path).await?;
        let hash = crate::checksum::compute_sha256(&self.dest_path).await.ok();
        Ok(hash)
    }
}

#[async_trait]
impl cloud_sync_core::ServerSideOperations for LocalSimulation {
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.copy(from, to).await
    }
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.rename(from, to).await
    }
}

#[async_trait]
impl cloud_sync_core::ResumableUploader for LocalSimulation {
    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn cloud_sync_core::ResumableUploadSession>, StorageError> {
        let meta = tokio::fs::metadata(local_path).await?;
        let dest = self.resolve(remote_path);
        Ok(Box::new(SimulatedResumableUploadSession {
            local_path: local_path.to_path_buf(),
            dest_path: dest,
            total_bytes: meta.len(),
            uploaded_bytes: 0,
            chunk_size: chunk_size.max(1024),
        }))
    }
}

#[async_trait]
impl StorageBackend for LocalSimulation {
    fn name(&self) -> &str {
        &self.provider_name
    }

    fn capabilities(&self) -> cloud_sync_core::StorageCapabilities {
        cloud_sync_core::StorageCapabilities::SERVER_SIDE_MOVE
            .union(cloud_sync_core::StorageCapabilities::SERVER_SIDE_COPY)
            .union(cloud_sync_core::StorageCapabilities::RESUMABLE_UPLOAD)
            .union(cloud_sync_core::StorageCapabilities::CHECKSUM_SHA256)
    }

    fn as_server_side_ops(&self) -> Option<&dyn cloud_sync_core::ServerSideOperations> {
        Some(self)
    }

    fn as_resumable_uploader(&self) -> Option<&dyn cloud_sync_core::ResumableUploader> {
        Some(self)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        self.upload(local_path, remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.download(remote_path, local_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.delete(remote_path).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.list(remote_path).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.rename(from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.copy(from, to).await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn cloud_sync_core::ResumableUploadSession>, StorageError> {
        let meta = tokio::fs::metadata(local_path).await?;
        let dest = self.resolve(remote_path);
        Ok(Box::new(SimulatedResumableUploadSession {
            local_path: local_path.to_path_buf(),
            dest_path: dest,
            total_bytes: meta.len(),
            uploaded_bytes: 0,
            chunk_size: chunk_size.max(1024),
        }))
    }

    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        if let Ok(meta) = tokio::fs::metadata(local_path).await {
            let size = meta.len();
            let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());
            return Ok(self.get_or_compute_checksum(local_path, size, modified).await);
        }
        Ok(crate::checksum::compute_sha256(local_path).await.ok())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn simulation_reason(&self) -> Option<String> {
        Some(format!(
            "Local directory simulation ({})",
            self.root_dir.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_local_sim_errors_and_edge_cases() {
        let temp_dir = tempdir().unwrap();
        let sim = LocalSimulation::new(temp_dir.path().to_path_buf(), "MockLocal".to_string())
            .with_limiters(None, None);

        // 1. Download non-existent file
        let download_dest = temp_dir.path().join("download_dest.txt");
        let res = sim.download("missing.txt", &download_dest).await;
        assert!(matches!(res, Err(StorageError::NotFound(_))));

        // 2. Delete non-existent file
        let res = sim.delete("missing.txt").await;
        assert!(matches!(res, Err(StorageError::NotFound(_))));

        // 3. Delete directory
        let dir_to_delete = temp_dir.path().join("dir_to_del");
        std::fs::create_dir(&dir_to_delete).unwrap();
        std::fs::write(dir_to_delete.join("file.txt"), "data").unwrap();
        sim.delete("dir_to_del").await.unwrap();
        assert!(!dir_to_delete.exists());

        // 4. Rename non-existent file
        let res = sim.rename("missing.txt", "new.txt").await;
        assert!(matches!(res, Err(StorageError::NotFound(_))));

        // 5. Rename with non-existent destination parent dir
        let src_file = temp_dir.path().join("src.txt");
        std::fs::write(&src_file, "data").unwrap();
        sim.upload(&src_file, "src.txt").await.unwrap();
        sim.rename("src.txt", "new_parent/dest.txt").await.unwrap();
        assert!(temp_dir.path().join("new_parent/dest.txt").exists());

        // 6. List non-existent directory
        let res = sim.list("missing_dir").await;
        assert!(matches!(res, Err(StorageError::NotFound(_))));

        // 7. compute_local_checksum
        let dest_file = temp_dir.path().join("new_parent/dest.txt");
        let checksum = sim.compute_local_checksum(&dest_file).await.unwrap();
        assert!(checksum.is_some());
    }

    #[tokio::test]
    async fn test_local_sim_specification_compliance() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_root");
        tokio::fs::create_dir_all(&sim_root).await.unwrap();
        let sim = LocalSimulation::new(sim_root, "compliance_test".to_string());
        cloud_sync_core::verify_backend_compliance(&sim, dir.path())
            .await
            .expect("Specification compliance test failed");
    }

    #[tokio::test]
    async fn test_local_sim_metadata_checksum_cache() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_root");
        let sim = LocalSimulation::new(sim_root.clone(), "cache_test".to_string());

        let src = dir.path().join("src.txt");
        tokio::fs::write(&src, b"version_1").await.unwrap();
        sim.upload(&src, "doc.txt").await.unwrap();

        let items1 = sim.list("").await.unwrap();
        assert_eq!(items1.len(), 1);
        let hash1 = items1[0].checksum.clone().unwrap();

        // Cache should now contain 1 entry
        assert_eq!(sim.checksum_cache.read().unwrap().len(), 1);

        // Second list returns identical cached hash
        let items2 = sim.list("").await.unwrap();
        assert_eq!(items2[0].checksum.as_deref(), Some(hash1.as_str()));

        // Modify file with different content & size -> cache invalidates and updates
        tokio::fs::write(&src, b"version_2_longer_payload")
            .await
            .unwrap();
        sim.upload(&src, "doc.txt").await.unwrap();

        let items3 = sim.list("").await.unwrap();
        let hash3 = items3[0].checksum.clone().unwrap();
        assert_ne!(hash1, hash3);
    }

    #[tokio::test]
    async fn test_local_sim_path_traversal_defense() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_root");
        let sim = LocalSimulation::new(sim_root.clone(), "traversal_test".to_string());

        let local_src = dir.path().join("test.txt");
        tokio::fs::write(&local_src, b"danger").await.unwrap();

        // Path traversal attempts must be rejected with InvalidPath
        assert!(sim.upload(&local_src, "../../escaped.txt").await.is_err());
        assert!(sim.upload(&local_src, "foo/../../escaped.txt").await.is_err());
        assert!(sim.upload(&local_src, "nul").await.is_err());
        assert!(sim.upload(&local_src, "file.txt:stream").await.is_err());

        let dest = dir.path().join("out.txt");
        assert!(sim.download("../../secret.txt", &dest).await.is_err());
        assert!(sim.delete("../../secret.txt").await.is_err());
    }
}
