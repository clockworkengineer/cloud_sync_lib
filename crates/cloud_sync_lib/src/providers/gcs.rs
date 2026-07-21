//! Google Cloud Storage (GCS) storage backend provider implementation.
//!
//! Handles interaction with the GCS JSON REST API. Supports OAuth2 authentication
//! using Service Account JSON key files or bypasses auth for local emulators.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::GCSCredentials;
use crate::providers::utils::translate_http_error;
use async_trait::async_trait;
use std::path::Path;
use std::time::SystemTime;
use tokio::fs;
use tracing::info;
use serde::Deserialize;
use yup_oauth2::authenticator::ServiceAccountAuthenticator;

#[derive(Deserialize, Debug)]
struct GCSObject {
    name: String,
    size: String,
    updated: String,
}

#[derive(Deserialize, Debug)]
struct GCSListResponse {
    items: Option<Vec<GCSObject>>,
}

/// Storage provider client for Google Cloud Storage JSON API.
pub struct GCSProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: GCSCredentials,
    /// GCS base API URL.
    api_url: String,
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

impl GCSProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: GCSCredentials) -> GCSProviderBuilder {
        GCSProviderBuilder::new(credentials)
    }

    /// Creates a new `GCSProvider` using the provided credentials.
    pub fn new(credentials: GCSCredentials) -> Self {
        Self::with_client_options(credentials, None, None)
    }

    /// Creates a new `GCSProvider` with custom HTTP client options.
    pub fn with_client_options(
        credentials: GCSCredentials,
        timeout: Option<std::time::Duration>,
        custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        let api_url = if let Some(ref ep) = credentials.endpoint {
            ep.trim_end_matches('/').to_string()
        } else {
            "https://storage.googleapis.com".to_string()
        };

        Self {
            client: super::utils::build_http_client(timeout, custom_headers),
            credentials,
            api_url,
        }
    }

    /// Configures custom endpoints, useful for mocking during tests.
    #[cfg(test)]
    pub fn with_endpoints(mut self, api_url: String) -> Self {
        self.api_url = api_url;
        self
    }

    fn format_path<'a>(&self, remote_path: &'a str) -> std::borrow::Cow<'a, str> {
        crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }

    /// Retrieves an access token for GCS if service account credentials are provided.
    async fn get_access_token(&self) -> Result<Option<String>, StorageError> {
        if self.credentials.service_account_key_path.is_empty() {
            return Ok(None);
        }

        let key = yup_oauth2::read_service_account_key(&self.credentials.service_account_key_path)
            .await
            .map_err(|e| StorageError::Authentication(format!("Failed to read service account key: {}", e)))?;

        let auth = ServiceAccountAuthenticator::builder(key)
            .build()
            .await
            .map_err(|e| StorageError::Authentication(format!("Failed to build authenticator: {}", e)))?;

        let token = auth
            .token(&["https://www.googleapis.com/auth/devstorage.full_control"])
            .await
            .map_err(|e| StorageError::Authentication(format!("Failed to retrieve OAuth token: {}", e)))?;

        Ok(Some(token.token().unwrap_or_default().to_string()))
    }
}

#[async_trait]
impl StorageBackend for GCSProvider {
    fn name(&self) -> &str {
        "GCS"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let clean_path = self.format_path(remote_path);
            info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

            let file_content = fs::read(local_path).await?;
            let token = self.get_access_token().await?;

            // URL encode the object name
            let encoded_name = url_encode(&clean_path);
            let upload_url = format!(
                "{}/upload/storage/v1/b/{}/o?uploadType=media&name={}",
                self.api_url, self.credentials.bucket, encoded_name
            );

            let mut req = self.client.post(&upload_url)
                .header("Content-Type", "application/octet-stream")
                .body(file_content);

            if let Some(tok) = &token {
                req = req.bearer_auth(tok);
            }

            let res = req.send().await?;

            if !res.status().is_success() {
                return Err(translate_http_error(res, self.name(), "upload").await);
            }

            Ok(())
        }).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "download", || async {
            let clean_path = self.format_path(remote_path);
            let token = self.get_access_token().await?;

            let encoded_name = url_encode(&clean_path);
            let download_url = format!(
                "{}/storage/v1/b/{}/o/{}?alt=media",
                self.api_url, self.credentials.bucket, encoded_name
            );

            let mut req = self.client.get(&download_url);
            if let Some(tok) = &token {
                req = req.bearer_auth(tok);
            }

            let res = req.send().await?;

            if !res.status().is_success() {
                return Err(translate_http_error(res, self.name(), "download").await);
            }

            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            let bytes = res.bytes().await?;
            fs::write(local_path, bytes).await?;
            Ok(())
        }).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "delete", || async {
            let clean_path = self.format_path(remote_path);
            let token = self.get_access_token().await?;

            let encoded_name = url_encode(&clean_path);
            let delete_url = format!(
                "{}/storage/v1/b/{}/o/{}",
                self.api_url, self.credentials.bucket, encoded_name
            );

            let mut req = self.client.delete(&delete_url);
            if let Some(tok) = &token {
                req = req.bearer_auth(tok);
            }

            let res = req.send().await?;

            if !res.status().is_success() {
                return Err(translate_http_error(res, self.name(), "delete").await);
            }

            Ok(())
        }).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let clean_path = self.format_path(remote_path).into_owned();
            let token = self.get_access_token().await?;

            let prefix_query = if clean_path.is_empty() {
                "".to_string()
            } else {
                // GCS expects prefix parameter
                let prefix = if clean_path.ends_with('/') {
                    clean_path.clone()
                } else {
                    format!("{}/", clean_path)
                };
                format!("&prefix={}", url_encode(&prefix))
            };

            let list_url = format!(
                "{}/storage/v1/b/{}/o?{}",
                self.api_url, self.credentials.bucket, prefix_query
            );

            let mut req = self.client.get(&list_url);
            if let Some(tok) = &token {
                req = req.bearer_auth(tok);
            }

            let res = req.send().await?;

            if !res.status().is_success() {
                return Err(translate_http_error(res, self.name(), "list").await);
            }

            let list_response: GCSListResponse = res.json().await?;
            let mut items = Vec::new();

            if let Some(objects) = list_response.items {
                for obj in objects {
                    let item_path = super::utils::strip_destination_prefix(Path::new(&obj.name), self.credentials.common.destination_folder.as_deref());

                    // Parse GCS RFC3339 time format
                    let modified = time::OffsetDateTime::parse(&obj.updated, &time::format_description::well_known::Rfc3339)
                        .map(SystemTime::from)
                        .unwrap_or(SystemTime::now());

                    let size = obj.size.parse::<u64>().unwrap_or(0);

                    items.push(StorageItem {
                        path: item_path,
                        size,
                        modified,
                        is_dir: false, // GCS is a flat namespace, virtual directories are simulated
                        checksum: None,
                        permissions: None,
                });
                }
            }

            Ok(items)
        }).await
    }

}


/// Builder for [`GCSProvider`].
pub struct GCSProviderBuilder {
    pub credentials: GCSCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl GCSProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: GCSCredentials) -> Self {
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
    pub fn build(self) -> GCSProvider {
        GCSProvider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}
