//! Dropbox storage backend provider implementation.
//!
//! Handles interaction with the Dropbox v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Storage provider client for Dropbox.
///
/// If credentials are provided, connects to the Dropbox v2 API endpoints.
/// If credentials are `None`, simulates behavior by reading/writing files
/// inside the local directory specified by `root_dir`.
pub struct DropboxProvider {
    root_dir: PathBuf,
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
    auth_url: String,
    api_url: String,
    content_url: String,
}

impl DropboxProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self {
            root_dir,
            client: reqwest::Client::new(),
            credentials,
            auth_url: "https://api.dropbox.com/oauth2/token".to_string(),
            api_url: "https://api.dropboxapi.com/2/files".to_string(),
            content_url: "https://content.dropboxapi.com/2/files".to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, content_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.content_url = content_url;
        self
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No Dropbox credentials configured".into())
        })?;

        let params = [
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", &"refresh_token".to_string()),
        ];

        let res = self.client.post(&self.auth_url)
            .form(&params)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let token = res["access_token"].as_str().ok_or_else(|| {
            StorageError::Authentication(format!("Failed to retrieve Dropbox access token: {:?}", res))
        })?;

        Ok(token.to_string())
    }

    fn format_path(&self, path: &str) -> String {
        let clean_path = path.trim_start_matches('/');
        let mut full_path = String::new();

        if let Some(ref creds) = self.credentials {
            if let Some(ref dest_folder) = creds.destination_folder {
                let clean_dest = dest_folder.trim_matches('/');
                if !clean_dest.is_empty() {
                    full_path.push_str("/");
                    full_path.push_str(clean_dest);
                }
            }
        }

        if !clean_path.is_empty() {
            full_path.push_str("/");
            full_path.push_str(clean_path);
        }

        full_path
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let destination = self.resolve(remote_path);
            info!("[{}] (Simulated) Uploading local file {:?} to remote path '{}'", self.name(), local_path, remote_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(local_path, &destination).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        info!("[{}] Real upload starting for '{}'", self.name(), dbx_path);
        let file_content = fs::read(local_path).await?;

        let api_arg = serde_json::json!({
            "path": dbx_path,
            "mode": "overwrite",
            "autorename": false,
            "mute": false,
            "strict_conflict": false
        });

        let upload_url = format!("{}/upload", self.content_url);
        let res = self.client.post(&upload_url)
            .bearer_auth(&token)
            .header("Dropbox-API-Arg", serde_json::to_string(&api_arg).unwrap())
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(StorageError::Provider(format!("Failed to upload to Dropbox: {}", body)));
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let source = self.resolve(remote_path);
            info!("[{}] (Simulated) Downloading remote path '{}' to local file {:?}", self.name(), remote_path, local_path);
            if !source.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source, local_path).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let api_arg = serde_json::json!({
            "path": dbx_path
        });

        let download_url = format!("{}/download", self.content_url);
        let res = self.client.post(&download_url)
            .bearer_auth(&token)
            .header("Dropbox-API-Arg", serde_json::to_string(&api_arg).unwrap())
            .header("Content-Type", "")
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to download from Dropbox: {}", res.status())));
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let bytes = res.bytes().await?;
        fs::write(local_path, bytes).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Deleting remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if target.is_dir() {
                fs::remove_dir_all(&target).await?;
            } else {
                fs::remove_file(&target).await?;
            }
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let body = serde_json::json!({
            "path": dbx_path
        });

        let delete_url = format!("{}/delete_v2", self.api_url);
        let res = self.client.post(&delete_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to delete on Dropbox: {}", res.status())));
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if self.credentials.is_none() {
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Listing contents of remote path '{}'", self.name(), remote_path);
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
            return Ok(items);
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let body = serde_json::json!({
            "path": dbx_path,
            "recursive": false
        });

        let list_url = format!("{}/list_folder", self.api_url);
        let res = self.client.post(&list_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let mut items = Vec::new();
        if let Some(entries) = res["entries"].as_array() {
            for entry in entries {
                let name = entry["name"].as_str().unwrap_or("").to_string();
                let size = entry["size"].as_u64().unwrap_or(0);
                let tag = entry[".tag"].as_str().unwrap_or("");
                let is_dir = tag == "folder";

                items.push(StorageItem {
                    path: PathBuf::from(name),
                    size,
                    modified: std::time::SystemTime::now(),
                    is_dir,
                });
            }
        }

        Ok(items)
    }
}
