//! Dropbox storage backend provider implementation.
//!
//! Handles interaction with the Dropbox v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;

/// Storage provider client for Dropbox REST API.
pub struct DropboxProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration (client id/secret, refresh token).
    credentials: OAuthCredentials,
    /// The base API URL.
    api_url: String,
    /// The base content API URL.
    content_url: String,
    /// Shared OAuth token manager.
    token_manager: std::sync::Arc<super::utils::OAuthTokenManager>,
    /// Optional upload rate limiter.
    upload_limiter: Option<crate::rate_limit::TokenBucket>,
    /// Optional download rate limiter.
    download_limiter: Option<crate::rate_limit::TokenBucket>,
}

impl DropboxProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: OAuthCredentials) -> DropboxProviderBuilder {
        DropboxProviderBuilder::new(credentials)
    }

    /// Creates a new `DropboxProvider` using the provided OAuth credentials.
    ///
    /// # Arguments
    /// * `credentials` - OAuth credentials and sync configuration.
    ///
    /// # Returns
    /// A new instance of `DropboxProvider`.
    pub fn new(credentials: OAuthCredentials) -> Self {
        let client = super::utils::build_http_client();
        let auth_url = "https://api.dropbox.com/oauth2/token".to_string();
        let token_manager = std::sync::Arc::new(super::utils::OAuthTokenManager::new(
            client.clone(),
            &auth_url,
            &credentials.client_id,
            &credentials.client_secret,
            &credentials.refresh_token,
            "Dropbox",
        ));
        let upload_limiter = credentials.common.max_upload_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        let download_limiter = credentials.common.max_download_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        Self {
            client,
            credentials,
            api_url: "https://api.dropboxapi.com/2/files".to_string(),
            content_url: "https://content.dropboxapi.com/2/files".to_string(),
            token_manager,
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
        self.api_url = api_url;
        self.content_url = content_url;
        self.token_manager = std::sync::Arc::new(super::utils::OAuthTokenManager::new(
            self.client.clone(),
            &auth_url,
            &self.credentials.client_id,
            &self.credentials.client_secret,
            &self.credentials.refresh_token,
            "Dropbox",
        ));
        self
    }

    /// Helper to retrieve a valid OAuth access token, refreshing it if necessary.
    ///
    /// # Returns
    /// The access token string, or a `StorageError` if authorization fails.
    async fn get_access_token(&self) -> Result<String, StorageError> {
        self.token_manager.get_access_token().await
    }

    /// Formats the remote path, incorporating the optional destination folder prefix.
    ///
    fn format_path<'a>(&self, path: &'a str) -> std::borrow::Cow<'a, str> {
        crate::providers::utils::format_absolute_path(path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
    }


    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let token = self.get_access_token().await?;
            let dbx_path = self.format_path(remote_path);

            info!("[{}] Real upload starting for '{}'", self.name(), dbx_path);
            let (body, size) = super::utils::get_upload_body(local_path, self.upload_limiter.clone()).await?;

            let client_modified = std::fs::metadata(local_path)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| {
                    let datetime = time::OffsetDateTime::from(t);
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                        datetime.year(),
                        datetime.month() as u8,
                        datetime.day(),
                        datetime.hour(),
                        datetime.minute(),
                        datetime.second()
                    )
                });

            let mut api_arg = serde_json::json!({
                "path": dbx_path,
                "mode": "overwrite",
                "autorename": false,
                "mute": false,
                "strict_conflict": false
            });

            if let Some(ref cm) = client_modified {
                if let Some(obj) = api_arg.as_object_mut() {
                    obj.insert("client_modified".to_string(), serde_json::Value::String(cm.clone()));
                }
            }

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
        }).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "download", || async {
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
        }).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "delete", || async {
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
        }).await
    }

    /// Creates a directory folder on Dropbox.
    ///
    /// # Arguments
    /// * `remote_path` - The folder path relative to the sync root.
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "create_folder", || async {
            let token = self.get_access_token().await?;
            let dbx_path = self.format_path(remote_path);

            let body = serde_json::json!({
                "path": dbx_path,
                "autorename": false
            });

            let create_url = format!("{}/create_folder_v2", self.api_url);
            let res = self.client.post(&create_url)
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() {
                let err_text = res.text().await.unwrap_or_default();
                if err_text.contains("path/conflict") {
                    return Ok(());
                }
                return Err(StorageError::Provider { message: format!("Failed to create folder: {}", err_text), status: Some(status.as_u16()) });
            }

            Ok(())
        }).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let token = self.get_access_token().await?;
            let dbx_path = self.format_path(remote_path);

            let mut items = Vec::new();
            let mut cursor: Option<String> = None;

            loop {
                let res = if let Some(ref cur) = cursor {
                    let body = serde_json::json!({
                        "cursor": cur
                    });
                    let continue_url = format!("{}/list_folder/continue", self.api_url);
                    self.client.post(&continue_url)
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await?
                        .json::<serde_json::Value>()
                        .await?
                } else {
                    let body = serde_json::json!({
                        "path": dbx_path,
                        "recursive": false
                    });
                    let list_url = format!("{}/list_folder", self.api_url);
                    self.client.post(&list_url)
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await?
                        .json::<serde_json::Value>()
                        .await?
                };

                if let Some(entries) = res["entries"].as_array() {
                    for entry in entries {
                        let name = entry["name"].as_str().unwrap_or("").to_string();
                        let size = entry["size"].as_u64().unwrap_or(0);
                        let tag = entry[".tag"].as_str().unwrap_or("");
                        let is_dir = tag == "folder";
                        let checksum = entry["content_hash"].as_str().map(|s| s.to_string());

                        let modified = entry["client_modified"].as_str()
                            .and_then(|t| time::OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).ok())
                            .map(std::time::SystemTime::from)
                            .unwrap_or_else(std::time::SystemTime::now);

                        let rel_path = if remote_path.is_empty() {
                            name
                        } else {
                            format!("{}/{}", remote_path, name)
                        };

                        items.push(StorageItem {
                            path: PathBuf::from(rel_path),
                            size,
                            modified,
                            is_dir,
                            checksum,
                            permissions: None,
                });
                    }
                }

                if res["has_more"].as_bool().unwrap_or(false) {
                    cursor = res["cursor"].as_str().map(|s| s.to_string());
                } else {
                    break;
                }
            }

            Ok(items)
        }).await
    }

    async fn compute_local_checksum(&self, local_path: &Path) -> Result<Option<String>, StorageError> {
        Ok(crate::checksum::compute_dropbox_hash(local_path).await.ok())
    }
}



/// Builder for [`DropboxProvider`].
pub struct DropboxProviderBuilder {
    pub credentials: OAuthCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl DropboxProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: OAuthCredentials) -> Self {
        Self {
            credentials,
            timeout: None,
            custom_headers: None,
        }
    }

    /// Configures the connection timeout.
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Configures custom HTTP headers.
    pub fn custom_headers(mut self, headers: reqwest::header::HeaderMap) -> Self {
        self.custom_headers = Some(headers);
        self
    }

    /// Builds the provider.
    pub fn build(self) -> DropboxProvider {
        DropboxProvider::new(self.credentials)
    }
}
