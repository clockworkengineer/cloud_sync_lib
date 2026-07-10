//! OneDrive storage backend provider implementation.
//!
//! Handles interaction with the Microsoft Graph REST API for OneDrive. Supports full OAuth2-based
//! upload, download, delete, and list operations.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;

/// Storage provider client for Microsoft OneDrive REST API.
pub struct OneDriveProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration (client id/secret, refresh token).
    credentials: OAuthCredentials,
    /// The authentication/token URL.
    auth_url: String,
    /// The base API URL.
    api_url: String,
    /// Optional upload rate limiter.
    upload_limiter: Option<crate::rate_limit::TokenBucket>,
    /// Optional download rate limiter.
    download_limiter: Option<crate::rate_limit::TokenBucket>,
}

impl OneDriveProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: OAuthCredentials) -> OneDriveProviderBuilder {
        OneDriveProviderBuilder::new(credentials)
    }

    /// Creates a new `OneDriveProvider` using the provided OAuth credentials.
    ///
    /// # Arguments
    /// * `credentials` - OAuth credentials and sync configuration.
    ///
    /// # Returns
    /// A new instance of `OneDriveProvider`.
    pub fn new(credentials: OAuthCredentials) -> Self {
        let upload_limiter = credentials.common.max_upload_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        let download_limiter = credentials.common.max_download_rate.map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024));
        Self {
            client: super::utils::build_http_client(),
            credentials,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
            api_url: "https://graph.microsoft.com/v1.0".to_string(),
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
    ///
    /// # Returns
    /// The modified `OneDriveProvider` instance.
    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
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
    fn format_path(&self, remote_path: &str) -> String {
        crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for OneDriveProvider {
    fn name(&self) -> &str {
        "OneDrive"
    }


    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let token = self.get_access_token().await?;
            let clean_path = self.format_path(remote_path);

            info!("[{}] Real upload starting for '{}'", self.name(), clean_path);
            let (body, size) = super::utils::get_upload_body(local_path, self.upload_limiter.clone()).await?;

            let upload_url = format!("{}/me/drive/root:/{}:/content", self.api_url, clean_path);
            let res = self.client.put(&upload_url)
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
        }).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "download", || async {
            let token = self.get_access_token().await?;
            let clean_path = self.format_path(remote_path);

            let download_url = format!("{}/me/drive/root:/{}:/content", self.api_url, clean_path);
            let res = self.client.get(&download_url)
                .bearer_auth(&token)
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
            let clean_path = self.format_path(remote_path);

            let delete_url = format!("{}/me/drive/root:/{}", self.api_url, clean_path);
            let res = self.client.delete(&delete_url)
                .bearer_auth(&token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "delete").await);
            }

            Ok(())
        }).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let token = self.get_access_token().await?;
            let clean_path = self.format_path(remote_path);

            let mut list_url = if clean_path.is_empty() {
                format!("{}/me/drive/root/children", self.api_url)
            } else {
                format!("{}/me/drive/root:/{}:/children", self.api_url, clean_path)
            };

            let mut items = Vec::new();
            loop {
                let res = self.client.get(&list_url)
                    .bearer_auth(&token)
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

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
                            checksum: None,
                        });
                    }
                }

                if let Some(next_link) = res["@odata.nextLink"].as_str() {
                    list_url = next_link.to_string();
                } else {
                    break;
                }
            }

            Ok(items)
        }).await
    }

}



/// Builder for [`OneDriveProvider`].
pub struct OneDriveProviderBuilder {
    pub credentials: OAuthCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl OneDriveProviderBuilder {
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
    pub fn build(self) -> OneDriveProvider {
        OneDriveProvider::new(self.credentials)
    }
}
