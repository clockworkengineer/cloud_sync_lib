use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// A OneDrive storage provider that can sync either to a real API (if credentials are set)
/// or fall back to a local folder simulation.
pub struct OneDriveProvider {
    root_dir: PathBuf,
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
}

impl OneDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self {
            root_dir,
            client: reqwest::Client::new(),
            credentials,
        })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No OneDrive credentials configured".into())
        })?;

        let params = [
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", &"refresh_token".to_string()),
        ];

        let res = self.client.post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
            .form(&params)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let token = res["access_token"].as_str().ok_or_else(|| {
            StorageError::Authentication(format!("Failed to retrieve OneDrive access token: {:?}", res))
        })?;

        Ok(token.to_string())
    }
}

#[async_trait]
impl StorageBackend for OneDriveProvider {
    fn name(&self) -> &str {
        "OneDrive"
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
        let clean_path = remote_path.trim_start_matches('/');

        info!("[{}] Real upload starting for '{}'", self.name(), clean_path);
        let file_content = fs::read(local_path).await?;

        let upload_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content", clean_path);
        let res = self.client.put(&upload_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(StorageError::Provider(format!("Failed to upload to OneDrive: {}", body)));
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
        let clean_path = remote_path.trim_start_matches('/');

        let download_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content", clean_path);
        let res = self.client.get(&download_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to download from OneDrive: {}", res.status())));
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
        let clean_path = remote_path.trim_start_matches('/');

        let delete_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}", clean_path);
        let res = self.client.delete(&delete_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to delete on OneDrive: {}", res.status())));
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
        let clean_path = remote_path.trim_start_matches('/');

        let list_url = if clean_path.is_empty() {
            "https://graph.microsoft.com/v1.0/me/drive/root/children".to_string()
        } else {
            format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/children", clean_path)
        };

        let res = self.client.get(&list_url)
            .bearer_auth(&token)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let mut items = Vec::new();
        if let Some(values) = res["value"].as_array() {
            for item in values {
                let name = item["name"].as_str().unwrap_or("").to_string();
                let size = item["size"].as_u64().unwrap_or(0);
                let is_dir = item.get("folder").is_some();

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
