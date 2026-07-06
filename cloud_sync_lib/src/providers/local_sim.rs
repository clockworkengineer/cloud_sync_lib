//! Local folder fallback simulator.
//!
//! Provides an implementation of storage simulation on the local filesystem.

use crate::traits::{StorageBackend, StorageItem, StorageError};
use async_trait::async_trait;
use crate::rate_limit::{TokenBucket, copy_rate_limited};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

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
        Self {
            root_dir,
            provider_name,
            upload_limiter: None,
            download_limiter: None,
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

    /// Maps a remote path to the local directory simulation structure.
    ///
    /// # Arguments
    /// * `remote_path` - The remote path to resolve.
    ///
    /// # Returns
    /// The resolved absolute/relative `PathBuf` under `root_dir`.
    pub fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    /// Simulates uploading a file by copying it to the local simulation folder.
    ///
    /// # Arguments
    /// * `local_path` - The path to the file on the local machine.
    /// * `remote_path` - The simulated destination path.
    ///
    /// # Returns
    /// An empty `Result`, or a `StorageError` if copying fails.
    pub async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let destination = self.resolve(remote_path);
        info!("[{}] (Simulated) Uploading local file {:?} to remote path '{}'", self.provider_name, local_path, remote_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        copy_rate_limited(local_path, &destination, self.upload_limiter.clone()).await?;
        Ok(())
    }

    /// Simulates downloading a file by copying it from the local simulation folder.
    ///
    /// # Arguments
    /// * `remote_path` - The simulated source path to download from.
    /// * `local_path` - The destination path on the local machine.
    ///
    /// # Returns
    /// An empty `Result`, or a `StorageError` if the file doesn't exist or copying fails.
    pub async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let source = self.resolve(remote_path);
        info!("[{}] (Simulated) Downloading remote path '{}' to local file {:?}", self.provider_name, remote_path, local_path);
        if !source.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        copy_rate_limited(&source, local_path, self.download_limiter.clone()).await?;
        Ok(())
    }

    /// Simulates deleting a file or directory from the local simulation folder.
    ///
    /// # Arguments
    /// * `remote_path` - The simulated path to delete.
    ///
    /// # Returns
    /// An empty `Result`, or a `StorageError` if the file doesn't exist or deletion fails.
    pub async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve(remote_path);
        info!("[{}] (Simulated) Deleting remote path '{}'", self.provider_name, remote_path);
        if !target.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }
        if target.is_dir() {
            fs::remove_dir_all(&target).await?;
        } else {
            fs::remove_file(&target).await?;
        }
        Ok(())
    }

    /// Simulates creating a folder/directory in the local simulation folder.
    pub async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve(remote_path);
        info!("[{}] (Simulated) Creating remote directory '{}'", self.provider_name, remote_path);
        fs::create_dir_all(&target).await?;
        Ok(())
    }

    /// Simulates listing contents of the local simulation folder.
    ///
    /// # Arguments
    /// * `remote_path` - The simulated directory path to list.
    ///
    /// # Returns
    /// A vector of `StorageItem` containing metadata for files/folders in the directory, or a `StorageError`.
    pub async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target = self.resolve(remote_path);
        info!("[{}] (Simulated) Listing contents of remote path '{}'", self.provider_name, remote_path);
        if !target.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }
        let mut items = Vec::new();
        let mut entries = fs::read_dir(&target).await?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            items.push(StorageItem {
                path: entry.path().strip_prefix(&self.root_dir).unwrap_or(&entry.path()).to_path_buf(),
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(std::time::SystemTime::now()),
                is_dir: metadata.is_dir(),
            });
        }
        Ok(items)
    }
}

#[async_trait]
impl StorageBackend for LocalSimulation {
    fn name(&self) -> &str {
        &self.provider_name
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
}

