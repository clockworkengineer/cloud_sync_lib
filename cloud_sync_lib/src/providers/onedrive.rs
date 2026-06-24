//! OneDrive storage backend provider implementation.
//!
//! Handles interaction with the Microsoft Graph REST API for OneDrive. Supports full OAuth2-based
//! upload, download, delete, and list operations.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Storage provider client for Microsoft OneDrive REST API.
pub struct OneDriveProvider {
    client: reqwest::Client,
    credentials: OAuthCredentials,
}

impl OneDriveProvider {
    pub fn new(credentials: OAuthCredentials) -> Self {
        Self {
            client: reqwest::Client::new(),
            credentials,
        }
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        refresh_oauth2_token(
            &self.client,
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            &self.credentials.client_id,
            &self.credentials.client_secret,
            &self.credentials.refresh_token,
            self.name(),
        ).await
    }
}

#[async_trait]
impl StorageBackend for OneDriveProvider {
    fn name(&self) -> &str {
        "OneDrive"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
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
            return Err(parse_response_error(res, self.name(), "upload").await);
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        let download_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content", clean_path);
        let res = self.client.get(&download_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "download").await);
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let bytes = res.bytes().await?;
        fs::write(local_path, bytes).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        let delete_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}", clean_path);
        let res = self.client.delete(&delete_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "delete").await);
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
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
