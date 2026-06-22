use crate::traits::{StorageBackend, StorageError, StorageItem};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// A simulated Google Drive storage provider that copies files to/from a local directory.
pub struct GoogleDriveProvider {
    root_dir: PathBuf,
}

impl GoogleDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self { root_dir })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }
}

#[async_trait]
impl StorageBackend for GoogleDriveProvider {
    fn name(&self) -> &str {
        "Google Drive"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let destination = self.resolve(remote_path);
        info!(
            "[{}] Uploading local file {:?} to remote path '{}'",
            self.name(),
            local_path,
            remote_path
        );

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(local_path, &destination).await?;
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let source = self.resolve(remote_path);
        info!(
            "[{}] Downloading remote path '{}' to local file {:?}",
            self.name(),
            remote_path,
            local_path
        );

        if !source.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(&source, local_path).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Deleting remote path '{}'",
            self.name(),
            remote_path
        );

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

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Listing contents of remote path '{}'",
            self.name(),
            remote_path
        );

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

/// A simulated Dropbox storage provider that copies files to/from a local directory.
pub struct DropboxProvider {
    root_dir: PathBuf,
}

impl DropboxProvider {
    pub async fn new(root_dir: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self { root_dir })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let destination = self.resolve(remote_path);
        info!(
            "[{}] Uploading local file {:?} to remote path '{}'",
            self.name(),
            local_path,
            remote_path
        );

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(local_path, &destination).await?;
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let source = self.resolve(remote_path);
        info!(
            "[{}] Downloading remote path '{}' to local file {:?}",
            self.name(),
            remote_path,
            local_path
        );

        if !source.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(&source, local_path).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Deleting remote path '{}'",
            self.name(),
            remote_path
        );

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

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Listing contents of remote path '{}'",
            self.name(),
            remote_path
        );

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

/// A simulated OneDrive storage provider that copies files to/from a local directory.
pub struct OneDriveProvider {
    root_dir: PathBuf,
}

impl OneDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self { root_dir })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }
}

#[async_trait]
impl StorageBackend for OneDriveProvider {
    fn name(&self) -> &str {
        "OneDrive"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let destination = self.resolve(remote_path);
        info!(
            "[{}] Uploading local file {:?} to remote path '{}'",
            self.name(),
            local_path,
            remote_path
        );

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(local_path, &destination).await?;
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let source = self.resolve(remote_path);
        info!(
            "[{}] Downloading remote path '{}' to local file {:?}",
            self.name(),
            remote_path,
            local_path
        );

        if !source.exists() {
            return Err(StorageError::NotFound(remote_path.to_string()));
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(&source, local_path).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Deleting remote path '{}'",
            self.name(),
            remote_path
        );

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

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target = self.resolve(remote_path);
        info!(
            "[{}] Listing contents of remote path '{}'",
            self.name(),
            remote_path
        );

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
