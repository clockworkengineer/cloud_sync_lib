//! SMB / CIFS Network Share Provider (Rclone `smb` & FreeFileSync network share equivalent).
//!
//! Provides synchronization with Windows / Samba / NAS SMB network shares via UNC paths
//! (`\\server\share`) or mounted CIFS share points, supporting atomic file transfers,
//! server-side move/copy, and MD5 checksum calculation.

use crate::providers::SmbCredentials;
use crate::traits::{StorageBackend, StorageCapabilities, StorageError, StorageItem};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::info;

/// Storage provider for SMB / CIFS network shares.
pub struct SmbProvider {
    credentials: SmbCredentials,
    share_root: PathBuf,
}

crate::impl_provider_builder!(SmbProvider, SmbProviderBuilder, SmbCredentials);

impl SmbProvider {
    pub fn with_client_options(
        credentials: SmbCredentials,
        _timeout: Option<std::time::Duration>,
        _custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        let share_root = if let Some(ref custom_mount) = credentials.mount_path {
            PathBuf::from(custom_mount)
        } else if cfg!(windows) {
            let server = credentials
                .server
                .trim_start_matches('\\')
                .trim_start_matches('/');
            let share = credentials.share.trim_matches('\\').trim_matches('/');
            PathBuf::from(format!(r"\\{}\{}", server, share))
        } else {
            let server = credentials.server.trim_matches('/');
            let share = credentials.share.trim_matches('/');
            PathBuf::from(format!("/mnt/smb/{}/{}", server, share))
        };

        Self {
            credentials,
            share_root,
        }
    }

    fn resolve_share_path(&self, remote_path: &str) -> PathBuf {
        let rel = crate::providers::utils::format_relative_path(
            remote_path,
            self.credentials.common.destination_folder.as_deref(),
        );
        let clean = rel.trim_matches('/').trim_matches('\\');
        if clean.is_empty() {
            self.share_root.clone()
        } else {
            let mut p = self.share_root.clone();
            for segment in clean.split('/').filter(|s| !s.is_empty() && *s != "..") {
                p.push(segment);
            }
            p
        }
    }
}

#[async_trait]
impl StorageBackend for SmbProvider {
    fn name(&self) -> &str {
        "SMB"
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let target_dir = self.resolve_share_path(remote_path);
        if !target_dir.exists() {
            return Err(StorageError::NotFound(format!(
                "SMB share path not found or unreachable: {}",
                target_dir.display()
            )));
        }

        let mut entries = tokio::fs::read_dir(&target_dir).await?;
        let mut items = Vec::new();
        let clean_prefix = remote_path.trim_matches('/');

        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".smb_tmp") {
                continue;
            }

            let rel_path = if clean_prefix.is_empty() {
                file_name
            } else {
                format!("{}/{}", clean_prefix, file_name)
            };

            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let checksum = if meta.is_file() {
                crate::checksum::compute_md5(&entry.path()).await.ok()
            } else {
                None
            };

            items.push(StorageItem {
                path: PathBuf::from(rel_path),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified,
                is_dir: meta.is_dir(),
                checksum,
                permissions: None,
            });
        }

        Ok(items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let dest = self.resolve_share_path(remote_path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Atomic write via temporary file on the SMB share
        let tmp_dest = dest.with_extension("smb_tmp");
        tokio::fs::copy(local_path, &tmp_dest).await?;
        tokio::fs::rename(&tmp_dest, &dest).await?;

        info!("Uploaded file to SMB share: {}", remote_path);
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let src = self.resolve_share_path(remote_path);
        if !src.exists() {
            return Err(StorageError::NotFound(format!(
                "SMB remote file not found: {}",
                src.display()
            )));
        }

        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::copy(&src, local_path).await?;
        info!("Downloaded file from SMB share: {}", remote_path);
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve_share_path(remote_path);
        if !target.exists() {
            return Err(StorageError::NotFound(format!(
                "SMB target not found: {}",
                target.display()
            )));
        }

        if target.is_dir() {
            tokio::fs::remove_dir_all(&target).await?;
        } else {
            tokio::fs::remove_file(&target).await?;
        }
        Ok(())
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let target = self.resolve_share_path(remote_path);
        tokio::fs::create_dir_all(&target).await?;
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let src = self.resolve_share_path(from);
        let dst = self.resolve_share_path(to);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&src, &dst).await?;
        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let src = self.resolve_share_path(from);
        let dst = self.resolve_share_path(to);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&src, &dst).await?;
        Ok(())
    }

    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        Ok(crate::checksum::compute_md5(local_path).await.ok())
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::SERVER_SIDE_MOVE
            .union(StorageCapabilities::SERVER_SIDE_COPY)
            .union(StorageCapabilities::CHECKSUM_MD5)
    }
}

/// Builder for [`SmbProvider`].
pub struct SmbProviderBuilder {
    pub credentials: SmbCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl SmbProviderBuilder {
    pub fn new(credentials: SmbCredentials) -> Self {
        Self {
            credentials,
            timeout: None,
            custom_headers: None,
        }
    }

    pub fn build(self) -> SmbProvider {
        SmbProvider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_smb_provider_share_operations() {
        let share_dir = tempdir().unwrap();
        let local_dir = tempdir().unwrap();

        let creds = SmbCredentials {
            server: "nas.local".into(),
            share: "backups".into(),
            username: Some("user".into()),
            password: None,
            workgroup: Some("WORKGROUP".into()),
            mount_path: Some(share_dir.path().to_string_lossy().to_string()),
            common: Default::default(),
        };

        let provider = SmbProvider::new(creds);
        let src_file = local_dir.path().join("doc.txt");
        tokio::fs::write(&src_file, b"smb network share payload")
            .await
            .unwrap();

        provider.upload(&src_file, "folder/doc.txt").await.unwrap();

        let items = provider.list("folder").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].size, 25);
        assert!(items[0].checksum.is_some());

        provider
            .rename("folder/doc.txt", "folder/renamed.txt")
            .await
            .unwrap();
        provider
            .copy("folder/renamed.txt", "folder/copied.txt")
            .await
            .unwrap();

        let dl_path = local_dir.path().join("downloaded.txt");
        provider
            .download("folder/copied.txt", &dl_path)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(&dl_path).await.unwrap(),
            "smb network share payload"
        );

        provider.delete("folder/renamed.txt").await.unwrap();
        provider.delete("folder/copied.txt").await.unwrap();
    }
}
