//! MEGA storage backend provider implementation.
//!
//! Handles interaction with the MEGA.nz API using client-side encryption.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::MegaCredentials;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs::File;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Storage provider client for MEGA.nz.
pub struct MegaProvider {
    credentials: MegaCredentials,
}

impl MegaProvider {
    /// Creates a new `MegaProvider` using the provided credentials.
    pub fn new(credentials: MegaCredentials) -> Self {
        Self { credentials }
    }

    /// Helper to resolve a path to a MEGA Node by traversing the account hierarchy.
    ///
    /// If `create_folders` is true, missing intermediate folders will be created.
    async fn resolve_node(
        client: &mega::Client,
        path_str: &str,
        destination_folder: Option<&str>,
        create_folders: bool,
    ) -> Result<mega::Node, StorageError> {
        let nodes = client.fetch_own_nodes().await
            .map_err(|e| StorageError::Provider(e.to_string()))?;

        // Find the root node
        let mut current_node = nodes.iter()
            .find(|n| n.kind() == mega::NodeKind::Root)
            .cloned()
            .ok_or_else(|| StorageError::NotFound("MEGA root folder not found".to_string()))?;

        let mut segments = Vec::new();
        if let Some(dest) = destination_folder {
            let clean_dest = dest.trim_matches('/');
            if !clean_dest.is_empty() {
                for seg in clean_dest.split('/') {
                    segments.push(seg);
                }
            }
        }

        let clean_path = path_str.trim_start_matches('/');
        if !clean_path.is_empty() {
            for seg in clean_path.split('/') {
                segments.push(seg);
            }
        }

        if segments.is_empty() {
            return Ok(current_node);
        }

        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;
            
            // Find child node matching the segment name
            let child = current_node.children().iter()
                .filter_map(|child_hash| nodes.iter().find(|n| n.hash() == child_hash))
                .find(|n| n.name() == *segment)
                .cloned();

            match child {
                Some(node) => {
                    current_node = node;
                }
                None => {
                    if create_folders && (!is_last || segments.len() > 1) {
                        // Create folder
                        client.create_dir(&current_node, segment).await
                            .map_err(|e| StorageError::Provider(e.to_string()))?;
                        
                        // Re-fetch nodes to get the new folder's hash/node
                        let updated_nodes = client.fetch_own_nodes().await
                            .map_err(|e| StorageError::Provider(e.to_string()))?;
                        
                        current_node = updated_nodes.iter()
                            .filter_map(|n| {
                                if n.parent() == Some(current_node.hash()) && n.name() == *segment {
                                    Some(n.clone())
                                } else {
                                    None
                                }
                            })
                            .next()
                            .ok_or_else(|| StorageError::Provider("Failed to retrieve created folder".to_string()))?;
                    } else {
                        return Err(StorageError::NotFound(format!("Path segment '{}' not found", segment)));
                    }
                }
            }
        }

        Ok(current_node)
    }
}

#[async_trait]
impl StorageBackend for MegaProvider {
    fn name(&self) -> &str {
        "MEGA"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let email = self.credentials.email.clone();
        let password = self.credentials.password.clone();
        let dest_folder = self.credentials.common.destination_folder.clone();
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_string();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| StorageError::Provider(e.to_string()))?;

            rt.block_on(async {
                let mut client = mega::Client::builder()
                    .build(super::utils::build_http_client())
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                client.login(&email, &password, None).await
                    .map_err(|e| StorageError::Authentication(e.to_string()))?;

                // Determine destination folder and file name
                let path = Path::new(&remote_path);
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| StorageError::Provider("Invalid remote file name".to_string()))?
                    .to_string();

                let parent_path = path.parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("");

                // Resolve parent directory (incorporating destination_folder prefix)
                let parent_node = Self::resolve_node(&client, parent_path, dest_folder.as_deref(), true).await?;

                // If file already exists, delete it first to prevent duplicates
                let nodes = client.fetch_own_nodes().await
                    .map_err(|e| StorageError::Provider(e.to_string()))?;
                
                let existing = parent_node.children().iter()
                    .filter_map(|h| nodes.iter().find(|n| n.hash() == h))
                    .find(|n| n.name() == file_name);

                if let Some(node) = existing {
                    client.delete_node(node).await
                        .map_err(|e| StorageError::Provider(e.to_string()))?;
                }

                // Upload file (wrapping tokio::fs::File into compat futures_io::AsyncRead)
                let file = File::open(&local_path).await?;
                let metadata = file.metadata().await?;
                let size = metadata.len();
                let compat_file = file.compat();

                client.upload_node(&parent_node, &file_name, size, compat_file).await
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                Ok(())
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let email = self.credentials.email.clone();
        let password = self.credentials.password.clone();
        let dest_folder = self.credentials.common.destination_folder.clone();
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_string();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| StorageError::Provider(e.to_string()))?;

            rt.block_on(async {
                let mut client = mega::Client::builder()
                    .build(super::utils::build_http_client())
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                client.login(&email, &password, None).await
                    .map_err(|e| StorageError::Authentication(e.to_string()))?;

                let node = Self::resolve_node(&client, &remote_path, dest_folder.as_deref(), false).await?;
                if node.kind() != mega::NodeKind::File {
                    return Err(StorageError::Provider("Cannot download a directory".to_string()));
                }

                // Wrap tokio::fs::File into compat futures_io::AsyncWrite
                let file = File::create(&local_path).await?;
                let compat_file = file.compat_write();
                client.download_node(&node, compat_file).await
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                Ok(())
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let email = self.credentials.email.clone();
        let password = self.credentials.password.clone();
        let dest_folder = self.credentials.common.destination_folder.clone();
        let remote_path = remote_path.to_string();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| StorageError::Provider(e.to_string()))?;

            rt.block_on(async {
                let mut client = mega::Client::builder()
                    .build(super::utils::build_http_client())
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                client.login(&email, &password, None).await
                    .map_err(|e| StorageError::Authentication(e.to_string()))?;

                let node = Self::resolve_node(&client, &remote_path, dest_folder.as_deref(), false).await?;
                client.delete_node(&node).await
                    .map_err(|e| StorageError::Provider(e.to_string()))?;
                Ok(())
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let email = self.credentials.email.clone();
        let password = self.credentials.password.clone();
        let dest_folder = self.credentials.common.destination_folder.clone();
        let remote_path = remote_path.to_string();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| StorageError::Provider(e.to_string()))?;

            rt.block_on(async {
                let mut client = mega::Client::builder()
                    .build(super::utils::build_http_client())
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                client.login(&email, &password, None).await
                    .map_err(|e| StorageError::Authentication(e.to_string()))?;

                let folder_node = Self::resolve_node(&client, &remote_path, dest_folder.as_deref(), false).await?;
                
                let nodes = client.fetch_own_nodes().await
                    .map_err(|e| StorageError::Provider(e.to_string()))?;

                let items = folder_node.children().iter()
                    .filter_map(|hash| nodes.iter().find(|n| n.hash() == hash))
                    .map(|n| {
                        let is_dir = n.kind() == mega::NodeKind::Folder;
                        let relative_path = if remote_path.is_empty() {
                            PathBuf::from(n.name())
                        } else {
                            Path::new(&remote_path).join(n.name())
                        };

                        StorageItem {
                            path: relative_path,
                            size: n.size(),
                            modified: n.created_at()
                                .map(|dt| SystemTime::from(*dt))
                                .unwrap_or_else(SystemTime::now),
                            is_dir,
                            checksum: None,
                        }
                    })
                    .collect();

                Ok(items)
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    fn sync_mode(&self) -> super::SyncMode {
        use super::ProviderConfig;
        self.credentials.sync_mode()
    }
}
