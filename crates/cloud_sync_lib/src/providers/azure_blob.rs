//! Azure Blob Storage backend provider implementation.
//!
//! Handles interaction with Azure Blob Storage REST API using Shared Key authentication.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::AzureBlobCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tracing::info;
use ring::hmac;
use base64::Engine;
// Removed quick-xml imports

/// Storage provider client for Azure Blob Storage REST API.
pub struct AzureBlobProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: AzureBlobCredentials,
    /// The base API URL.
    api_url: String,
}

impl AzureBlobProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: AzureBlobCredentials) -> AzureBlobProviderBuilder {
        AzureBlobProviderBuilder::new(credentials)
    }

    /// Creates a new `AzureBlobProvider` using the provided credentials.
    pub fn new(credentials: AzureBlobCredentials) -> Self {
        let api_url = if let Some(ref ep) = credentials.endpoint {
            ep.trim_end_matches('/').to_string()
        } else {
            format!("https://{}.blob.core.windows.net", credentials.account_name)
        };

        Self {
            client: super::utils::build_http_client(),
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

    /// Signs an Azure REST request using Shared Key authorization.
    fn sign_request(
        &self,
        verb: &str,
        path_and_query: &str,
        content_length: Option<u64>,
        content_type: Option<&str>,
        additional_headers: &[(&str, &str)],
    ) -> Result<String, StorageError> {
        // Find x-ms-date and other x-ms- headers
        let mut ms_headers = Vec::new();
        for (k, v) in additional_headers {
            let kl = k.to_lowercase();
            if kl.starts_with("x-ms-") {
                ms_headers.push((kl, v.trim().to_string()));
            }
        }
        ms_headers.sort_by(|a, b| a.0.cmp(&b.0));

        let mut canonicalized_headers = String::new();
        for (k, v) in ms_headers {
            canonicalized_headers.push_str(&format!("{}:{}\n", k, v));
        }

        // CanonicalizedResource = "/accountname/containername/blobpath"
        // Parse query params if any
        let resource_path = if let Some(idx) = path_and_query.find('?') {
            &path_and_query[..idx]
        } else {
            path_and_query
        };

        let mut canonicalized_resource = format!("/{}/{}", self.credentials.account_name, self.credentials.container);
        if resource_path != "/" && !resource_path.is_empty() {
            canonicalized_resource.push_str(&format!("/{}", resource_path.trim_start_matches('/')));
        }

        if let Some(idx) = path_and_query.find('?') {
            let query = &path_and_query[idx + 1..];
            let mut params = Vec::new();
            for part in query.split('&') {
                let mut kv = part.splitn(2, '=');
                let k = kv.next().unwrap_or("");
                let v = kv.next().unwrap_or("");
                params.push((k.to_string(), v.to_string()));
            }
            params.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in params {
                // url decoding is omitted for simplicity in basic paths
                canonicalized_resource.push_str(&format!("\n{}:{}", k, v));
            }
        }

        let content_length_str = content_length.map(|l| l.to_string()).unwrap_or_default();
        let content_type_str = content_type.unwrap_or_default();

        let signable = format!(
            "{}\n\n\n{}\n\n{}\n\n\n\n\n\n\n{}{}",
            verb.to_uppercase(),
            content_length_str,
            content_type_str,
            canonicalized_headers,
            canonicalized_resource
        );

        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.credentials.account_key)
            .map_err(|e| StorageError::Authentication(format!("Invalid account key base64: {}", e)))?;

        let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
        let tag = hmac::sign(&key, signable.as_bytes());
        let signature = base64::engine::general_purpose::STANDARD.encode(tag.as_ref());

        Ok(format!("SharedKey {}:{}", self.credentials.account_name, signature))
    }
}

#[async_trait]
impl StorageBackend for AzureBlobProvider {
    fn name(&self) -> &str {
        "Azure Blob"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let clean_path = self.format_path(remote_path);
            info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

            let file_content = fs::read(local_path).await?;
            let len = file_content.len() as u64;

            let date_str = httpdate::fmt_http_date(SystemTime::now());
            let version_str = "2021-08-06";
            let blob_type = "BlockBlob";

            let path_and_query = format!("/{}", clean_path);
            let auth = self.sign_request(
                "PUT",
                &path_and_query,
                Some(len),
                Some("application/octet-stream"),
                &[
                    ("x-ms-date", &date_str),
                    ("x-ms-version", version_str),
                    ("x-ms-blob-type", blob_type),
                ],
            )?;

            let upload_url = format!("{}/{}/{}", self.api_url, self.credentials.container, clean_path);
            let res = self.client.put(&upload_url)
                .header("x-ms-date", &date_str)
                .header("x-ms-version", version_str)
                .header("x-ms-blob-type", blob_type)
                .header("Authorization", &auth)
                .header("Content-Type", "application/octet-stream")
                .body(file_content)
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
            let clean_path = self.format_path(remote_path);

            let date_str = httpdate::fmt_http_date(SystemTime::now());
            let version_str = "2021-08-06";

            let path_and_query = format!("/{}", clean_path);
            let auth = self.sign_request(
                "GET",
                &path_and_query,
                None,
                None,
                &[
                    ("x-ms-date", &date_str),
                    ("x-ms-version", version_str),
                ],
            )?;

            let download_url = format!("{}/{}/{}", self.api_url, self.credentials.container, clean_path);
            let res = self.client.get(&download_url)
                .header("x-ms-date", &date_str)
                .header("x-ms-version", version_str)
                .header("Authorization", &auth)
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
        }).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "delete", || async {
            let clean_path = self.format_path(remote_path);

            let date_str = httpdate::fmt_http_date(SystemTime::now());
            let version_str = "2021-08-06";

            let path_and_query = format!("/{}", clean_path);
            let auth = self.sign_request(
                "DELETE",
                &path_and_query,
                None,
                None,
                &[
                    ("x-ms-date", &date_str),
                    ("x-ms-version", version_str),
                ],
            )?;

            let delete_url = format!("{}/{}/{}", self.api_url, self.credentials.container, clean_path);
            let res = self.client.delete(&delete_url)
                .header("x-ms-date", &date_str)
                .header("x-ms-version", version_str)
                .header("Authorization", &auth)
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
            let clean_path = self.format_path(remote_path).into_owned();

            let date_str = httpdate::fmt_http_date(SystemTime::now());
            let version_str = "2021-08-06";

            // Query params must be sorted lexicographically in CanonicalizedResource
            // ?comp=list&prefix=prefix&restype=container
            let mut path_and_query = "/?comp=list&restype=container".to_string();
            if !clean_path.is_empty() {
                // Azure expects prefix to end with '/' to list a folder's content
                let prefix = if clean_path.ends_with('/') {
                    clean_path.clone()
                } else {
                    format!("{}/", clean_path)
                };
                path_and_query.push_str(&format!("&prefix={}", prefix));
            }

            let auth = self.sign_request(
                "GET",
                &path_and_query,
                None,
                None,
                &[
                    ("x-ms-date", &date_str),
                    ("x-ms-version", version_str),
                ],
            )?;

            let list_url = format!("{}/{}{}", self.api_url, self.credentials.container, &path_and_query[1..]);
            let res = self.client.get(&list_url)
                .header("x-ms-date", &date_str)
                .header("x-ms-version", version_str)
                .header("Authorization", &auth)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "list").await);
            }

            let body = res.text().await?;
            let mut items = Vec::new();
            let mut current_name = String::new();
            let mut current_size = 0;
            let mut current_modified = SystemTime::now();
            let mut active_tag = String::new();

            for token in xmlparser::Tokenizer::from(body.as_str()) {
                match token {
                    Ok(xmlparser::Token::ElementStart { local, .. }) => {
                        active_tag = local.as_str().to_string();
                    }
                    Ok(xmlparser::Token::ElementEnd { end, .. }) => {
                        match end {
                            xmlparser::ElementEnd::Close(_, local) => {
                                active_tag.clear();
                                let name = local.as_str();
                                if name == "Blob" {
                                    let mut item_path = PathBuf::from(&current_name);
                                    if let Some(ref dest_folder) = self.credentials.common.destination_folder {
                                        let clean_dest = dest_folder.trim_matches('/');
                                        if !clean_dest.is_empty() {
                                            if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
                                                item_path = stripped.to_path_buf();
                                            }
                                        }
                                    }

                                    items.push(StorageItem {
                                        path: item_path,
                                        size: current_size,
                                        modified: current_modified,
                                        is_dir: false,
                                        checksum: None,
                                        permissions: None,
                });
                                    current_name.clear();
                                    current_size = 0;
                                    current_modified = SystemTime::now();
                                }
                            }
                            xmlparser::ElementEnd::Empty => {
                                active_tag.clear();
                            }
                            _ => {}
                        }
                    }
                    Ok(xmlparser::Token::Text { text }) => {
                        let val = text.as_str().trim();
                        if !val.is_empty() {
                            if active_tag == "Name" {
                                current_name = val.to_string();
                            } else if active_tag == "Content-Length" {
                                current_size = val.parse::<u64>().unwrap_or(0);
                            } else if active_tag == "Last-Modified" {
                                if let Ok(time) = httpdate::parse_http_date(val) {
                                    current_modified = time;
                                }
                            }
                        }
                    }
                    Err(e) => return Err(StorageError::Provider { message: format!("XML parse error: {}", e), status: None }),
                    _ => {}
                }
            }

            Ok(items)
        }).await
    }

}


/// Builder for [`AzureBlobProvider`].
pub struct AzureBlobProviderBuilder {
    pub credentials: AzureBlobCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl AzureBlobProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: AzureBlobCredentials) -> Self {
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
    pub fn build(self) -> AzureBlobProvider {
        AzureBlobProvider::new(self.credentials)
    }
}
