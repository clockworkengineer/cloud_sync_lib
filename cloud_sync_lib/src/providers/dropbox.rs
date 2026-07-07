//! Dropbox storage backend provider implementation.
//!
//! Handles interaction with the Dropbox v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;

/// Storage provider client for Dropbox REST API.
pub struct DropboxProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration (client id/secret, refresh token).
    credentials: OAuthCredentials,
    /// The authentication/token URL.
    auth_url: String,
    /// The base API URL.
    api_url: String,
    /// The base content API URL.
    content_url: String,
    /// Optional upload rate limiter.
    upload_limiter: Option<crate::rate_limit::TokenBucket>,
    /// Optional download rate limiter.
    download_limiter: Option<crate::rate_limit::TokenBucket>,
}

impl DropboxProvider {
    /// Creates a new `DropboxProvider` using the provided OAuth credentials.
    ///
    /// # Arguments
    /// * `credentials` - OAuth credentials and sync configuration.
    ///
    /// # Returns
    /// A new instance of `DropboxProvider`.
    pub fn new(credentials: OAuthCredentials) -> Self {
        let upload_limiter = credentials.common.max_upload_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        let download_limiter = credentials.common.max_download_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        Self {
            client: super::utils::build_http_client(),
            credentials,
            auth_url: "https://api.dropbox.com/oauth2/token".to_string(),
            api_url: "https://api.dropboxapi.com/2/files".to_string(),
            content_url: "https://content.dropboxapi.com/2/files".to_string(),
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
    /// * `content_url` - Custom content API URL.
    ///
    /// # Returns
    /// The modified `DropboxProvider` instance.
    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, content_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.content_url = content_url;
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

    /// Formats the remote path, incorporating the optional destination folder prefix.
    ///
    fn format_path(&self, path: &str) -> String {
        crate::providers::utils::format_absolute_path(path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
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
        let dbx_path = self.format_path(remote_path);

        info!("[{}] Real upload starting for '{}'", self.name(), dbx_path);
        let (body, size) = super::utils::get_upload_body(local_path, self.upload_limiter.clone()).await?;

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
            return Err(parse_response_error(res, self.name(), "download").await);
        }

        super::utils::download_rate_limited(res, local_path, self.download_limiter.clone()).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
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
            return Err(parse_response_error(res, self.name(), "delete").await);
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
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

    fn sync_mode(&self) -> super::SyncMode {
        use super::ProviderConfig;
        self.credentials.sync_mode()
    }
}

