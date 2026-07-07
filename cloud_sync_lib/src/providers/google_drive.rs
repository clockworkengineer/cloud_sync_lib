//! Google Drive storage backend provider implementation.
//!
//! Handles interaction with the Google Drive API v3. Supports full OAuth2-based
//! upload, download, delete, and list operations, with recursive directory resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;

/// Storage provider client for Google Drive REST API.
pub struct GoogleDriveProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration (client id/secret, refresh token).
    credentials: OAuthCredentials,
    /// The authentication/token URL.
    auth_url: String,
    /// The base API URL.
    api_url: String,
    /// The base upload API URL.
    upload_url: String,
    /// Optional upload rate limiter.
    upload_limiter: Option<crate::rate_limit::TokenBucket>,
    /// Optional download rate limiter.
    download_limiter: Option<crate::rate_limit::TokenBucket>,
}

impl GoogleDriveProvider {
    /// Creates a new `GoogleDriveProvider` using the provided OAuth credentials.
    ///
    /// # Arguments
    /// * `credentials` - OAuth credentials and sync configuration.
    ///
    /// # Returns
    /// A new instance of `GoogleDriveProvider`.
    pub fn new(credentials: OAuthCredentials) -> Self {
        let upload_limiter = credentials.common.max_upload_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        let download_limiter = credentials.common.max_download_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        Self {
            client: super::utils::build_http_client(),
            credentials,
            auth_url: "https://oauth2.googleapis.com/token".to_string(),
            api_url: "https://www.googleapis.com/drive/v3/files".to_string(),
            upload_url: "https://www.googleapis.com/upload/drive/v3/files".to_string(),
            upload_limiter,
            download_limiter,
        }
    }

    /// Sets the upload and download rate limiters.
    pub fn with_limiters(
        mut self,
        upload_limiter: Option<crate::rate_limit::TokenBucket>,
        download_limiter: Option<crate::rate_limit::TokenBucket>,
    ) -> Self {
        if self.upload_limiter.is_none() {
            self.upload_limiter = upload_limiter;
        }
        if self.download_limiter.is_none() {
            self.download_limiter = download_limiter;
        }
        self
    }

    /// Configures custom endpoints, useful for mocking during tests.
    ///
    /// # Arguments
    /// * `auth_url` - Custom authorization URL.
    /// * `api_url` - Custom API URL.
    /// * `upload_url` - Custom upload API URL.
    ///
    /// # Returns
    /// The modified `GoogleDriveProvider` instance.
    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, upload_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.upload_url = upload_url;
        self
    }

    /// Helper to retrieve a valid OAuth access token, refreshing it if necessary.
    ///
    /// # Returns
    /// The access token string, or a `StorageError` if authorization fails.
    async fn get_access_token(&self) -> Result<String, StorageError> {
        refresh_oauth2_token(
            &self.client,
            &self.auth_url,
            &self.credentials.client_id,
            &self.credentials.client_secret,
            &self.credentials.refresh_token,
            self.name(),
        ).await
    }

    /// Retrieves or creates a Google Drive folder ID for a folder of the given name under `parent_id`.
    ///
    /// # Arguments
    /// * `token` - The active OAuth2 access token.
    /// * `parent_id` - The ID of the parent folder in Google Drive.
    /// * `name` - The name of the folder to resolve/create.
    ///
    /// # Returns
    /// The folder's ID, or a `StorageError`.
    async fn get_or_create_folder_id(&self, token: &str, parent_id: &str, name: &str) -> Result<String, StorageError> {
        let query = format!(
            "name = '{}' and '{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            name.replace('\'', "\\'"),
            parent_id
        );

        let res = self.client.get(&self.api_url)
            .bearer_auth(token)
            .query(&[("q", &query), ("fields", &"files(id)".to_string())])
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        if let Some(files) = res["files"].as_array() {
            if !files.is_empty() {
                return Ok(files[0]["id"].as_str().unwrap().to_string());
            }
        }

        let body = serde_json::json!({
            "name": name,
            "parents": [parent_id],
            "mimeType": "application/vnd.google-apps.folder"
        });

        let create_res = self.client.post(&self.api_url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let id = create_res["id"].as_str()
            .ok_or_else(|| StorageError::Provider(format!("Failed to create folder '{}' in Google Drive: {:?}", name, create_res)))?
            .to_string();

        Ok(id)
    }

    /// Resolves a path (e.g. "a/b/c.txt") to a Google Drive file ID.
    ///
    /// If directories in the path do not exist, they will be automatically created.
    ///
    /// # Arguments
    /// * `token` - The active OAuth2 access token.
    /// * `path` - The relative destination/source file path.
    /// * `is_folder` - True if we are resolving a folder path, false for a file.
    ///
    /// # Returns
    /// The ID of the target file/folder in Google Drive, or a `StorageError`.
    async fn get_or_create_file_id(&self, token: &str, path: &str, is_folder: bool) -> Result<String, StorageError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut parent_id = "root".to_string();

        if let Some(ref dest_folder) = self.credentials.common.destination_folder {
            if !dest_folder.is_empty() {
                parent_id = self.get_or_create_folder_id(token, &parent_id, dest_folder).await?;
            }
        }

        for (i, part) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;
            let current_is_folder = !is_last || is_folder;

            if current_is_folder {
                parent_id = self.get_or_create_folder_id(token, &parent_id, part).await?;
            } else {
                let query = format!(
                    "name = '{}' and '{}' in parents and mimeType != 'application/vnd.google-apps.folder' and trashed = false",
                    part.replace('\'', "\\'"),
                    parent_id
                );

                let res = self.client.get(&self.api_url)
                    .bearer_auth(token)
                    .query(&[("q", &query), ("fields", &"files(id)".to_string())])
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                if let Some(files) = res["files"].as_array() {
                    if !files.is_empty() {
                        parent_id = files[0]["id"].as_str().unwrap().to_string();
                        continue;
                    }
                }

                let body = serde_json::json!({
                    "name": part,
                    "parents": [parent_id],
                    "mimeType": "application/octet-stream"
                });

                let create_res = self.client.post(&self.api_url)
                    .bearer_auth(token)
                    .json(&body)
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                parent_id = create_res["id"].as_str()
                    .ok_or_else(|| StorageError::Provider(format!("Failed to create file '{}' in Google Drive: {:?}", part, create_res)))?
                    .to_string();
            }
        }

        Ok(parent_id)
    }
}

#[async_trait]
impl StorageBackend for GoogleDriveProvider {
    fn name(&self) -> &str {
        "Google Drive"
    }

    fn with_limiters(
        self,
        upload_limiter: Option<crate::rate_limit::TokenBucket>,
        download_limiter: Option<crate::rate_limit::TokenBucket>,
    ) -> Self
    where
        Self: Sized,
    {
        self.with_limiters(upload_limiter, download_limiter)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        info!("[{}] Real upload starting for '{}' (ID: {})", self.name(), remote_path, file_id);
        let (body, size) = super::utils::get_upload_body(local_path, self.upload_limiter.clone()).await?;
        
        let upload_url = format!("{}/{}?uploadType=media", self.upload_url, file_id);
        let res = self.client.patch(&upload_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", size.to_string())
            .body(body)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "upload").await);
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        let download_url = format!("{}/{}?alt=media", self.api_url, file_id);
        let res = self.client.get(&download_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "download").await);
        }

        super::utils::download_rate_limited(res, local_path, self.download_limiter.clone()).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        let delete_url = format!("{}/{}", self.api_url, file_id);
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
        let folder_id = self.get_or_create_file_id(&token, remote_path, true).await?;

        let query = format!("'{}' in parents and trashed = false", folder_id);
        let res = self.client.get(&self.api_url)
            .bearer_auth(&token)
            .query(&[("q", &query), ("fields", &"files(id, name, size, mimeType, modifiedTime, md5Checksum)".to_string())])
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let mut items = Vec::new();
        if let Some(files) = res["files"].as_array() {
            for file in files {
                let name = file["name"].as_str().unwrap_or("").to_string();
                let size = file["size"].as_str().unwrap_or("0").parse::<u64>().unwrap_or(0);
                let mime_type = file["mimeType"].as_str().unwrap_or("");
                let is_dir = mime_type == "application/vnd.google-apps.folder";
                let checksum = file["md5Checksum"].as_str().map(|s| s.to_string());

                items.push(StorageItem {
                    path: PathBuf::from(name),
                    size,
                    modified: std::time::SystemTime::now(),
                    is_dir,
                    checksum,
                });
            }
        }

        Ok(items)
    }

    fn sync_mode(&self) -> super::SyncMode {
        use super::ProviderConfig;
        self.credentials.sync_mode()
    }
}

