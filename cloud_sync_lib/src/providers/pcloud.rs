//! pCloud storage backend provider implementation.
//!
//! Handles interaction with the pCloud native JSON REST API. Supports OAuth2 authentication,
//! multipart uploads, download link resolution, file deletion, and listing.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::PCloudCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};
use tokio::fs;
use tracing::info;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct PCloudMetadata {
    name: String,
    size: Option<u64>,
    modified: Option<u64>,
    isfolder: bool,
}

#[derive(Deserialize, Debug)]
struct PCloudContents {
    contents: Option<Vec<PCloudMetadata>>,
}

#[derive(Deserialize, Debug)]
struct PCloudListResponse {
    metadata: Option<PCloudContents>,
}

#[derive(Deserialize, Debug)]
struct PCloudFileLinkResponse {
    hosts: Vec<String>,
    path: String,
}

/// Storage provider client for pCloud REST API.
pub struct PCloudProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: PCloudCredentials,
    /// pCloud API base URL.
    api_url: String,
}

impl PCloudProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: PCloudCredentials) -> PCloudProviderBuilder {
        PCloudProviderBuilder::new(credentials)
    }

    /// Creates a new `PCloudProvider` using the provided credentials.
    pub fn new(credentials: PCloudCredentials) -> Self {
        let api_url = if let Some(ref ep) = credentials.endpoint {
            ep.trim_end_matches('/').to_string()
        } else {
            "https://api.pcloud.com".to_string()
        };

        Self {
            client: super::utils::build_http_client(),
            credentials,
            api_url,
        }
    }

    fn format_path(&self, remote_path: &str) -> String {
        crate::providers::utils::format_absolute_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for PCloudProvider {
    fn name(&self) -> &str {
        "pCloud"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let clean_path = self.format_path(remote_path);
            info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

            let file_content = fs::read(local_path).await?;
            
            let parent_dir = Path::new(&clean_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string());

            let file_name = Path::new(&clean_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let upload_url = format!("{}/uploadfile", self.api_url);

            // Build multipart body
            let form = reqwest::multipart::Form::new()
                .part("file", reqwest::multipart::Part::bytes(file_content).file_name(file_name));

            let res = self.client.post(&upload_url)
                .bearer_auth(&self.credentials.access_token)
                .query(&[("path", &parent_dir)])
                .multipart(form)
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
            let link_url = format!("{}/getfilelink", self.api_url);

            let res = self.client.get(&link_url)
                .bearer_auth(&self.credentials.access_token)
                .query(&[("path", &clean_path)])
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "getfilelink").await);
            }

            let link_info: PCloudFileLinkResponse = res.json().await?;
            let host = link_info.hosts.first()
                .ok_or_else(|| StorageError::NotFound("No download hosts returned by pCloud".to_string()))?;

            let download_url = format!("https://{}{}", host, link_info.path);
            let dl_res = self.client.get(&download_url).send().await?;

            if !dl_res.status().is_success() {
                return Err(parse_response_error(dl_res, self.name(), "download").await);
            }

            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            let bytes = dl_res.bytes().await?;
            fs::write(local_path, bytes).await?;
            Ok(())
        }).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "delete", || async {
            let clean_path = self.format_path(remote_path);
            let delete_url = format!("{}/deletefile", self.api_url);

            let res = self.client.get(&delete_url)
                .bearer_auth(&self.credentials.access_token)
                .query(&[("path", &clean_path)])
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
            let clean_path = self.format_path(remote_path);
            let list_url = format!("{}/listfolder", self.api_url);

            let res = self.client.get(&list_url)
                .bearer_auth(&self.credentials.access_token)
                .query(&[("path", &clean_path)])
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "list").await);
            }

            let list_response: PCloudListResponse = res.json().await?;
            let mut items = Vec::new();

            if let Some(meta) = list_response.metadata {
                if let Some(contents) = meta.contents {
                    for entry in contents {
                        if entry.isfolder {
                            continue;
                        }

                        // Strip destination folder prefix from returned paths
                        let mut item_path = PathBuf::from(&entry.name);
                        if let Some(ref dest_folder) = self.credentials.common.destination_folder {
                            let clean_dest = dest_folder.trim_matches('/');
                            if !clean_dest.is_empty() {
                                if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
                                    item_path = stripped.to_path_buf();
                                }
                            }
                        }

                        let modified = entry.modified
                            .map(|m| SystemTime::UNIX_EPOCH + Duration::from_secs(m))
                            .unwrap_or(SystemTime::now());

                        items.push(StorageItem {
                            path: item_path,
                            size: entry.size.unwrap_or(0),
                            modified,
                            is_dir: false,
                            checksum: None,
                        });
                    }
                }
            }

            Ok(items)
        }).await
    }

}


/// Builder for [`PCloudProvider`].
pub struct PCloudProviderBuilder {
    pub credentials: PCloudCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl PCloudProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: PCloudCredentials) -> Self {
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
    pub fn build(self) -> PCloudProvider {
        PCloudProvider::new(self.credentials)
    }
}
