//! Backblaze B2 storage backend provider implementation.
//!
//! Handles interaction with the Backblaze B2 native JSON REST API. Supports authorization,
//! upload url retrieval, file upload, download, deletion, and listing.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::B2Credentials;
use crate::providers::utils::translate_http_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, Duration};
use tokio::fs;
use tracing::info;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
struct B2Auth {
    account_id: String,
    auth_token: String,
    api_url: String,
    download_url: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct B2AuthorizeResponse {
    account_id: String,
    authorization_token: String,
    api_url: String,
    download_url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ListBucketsRequest {
    account_id: String,
    bucket_name: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct B2Bucket {
    bucket_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ListBucketsResponse {
    buckets: Vec<B2Bucket>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GetUploadUrlRequest {
    bucket_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct B2UploadUrlResponse {
    upload_url: String,
    authorization_token: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ListFileNamesRequest {
    bucket_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_file_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct B2File {
    file_id: String,
    file_name: String,
    content_length: u64,
    upload_timestamp: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ListFileNamesResponse {
    files: Vec<B2File>,
    next_file_name: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeleteFileVersionRequest {
    file_name: String,
    file_id: String,
}

/// Storage provider client for Backblaze B2 REST API.
pub struct B2Provider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: B2Credentials,
    /// Cached auth data.
    auth_cache: Mutex<Option<B2Auth>>,
    /// Cached bucket ID.
    bucket_id_cache: Mutex<Option<String>>,
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

impl B2Provider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: B2Credentials) -> B2ProviderBuilder {
        B2ProviderBuilder::new(credentials)
    }

    /// Creates a new `B2Provider` using the provided credentials.
    pub fn new(credentials: B2Credentials) -> Self {
        Self::with_client_options(credentials, None, None)
    }

    /// Creates a new `B2Provider` with custom HTTP client options.
    pub fn with_client_options(
        credentials: B2Credentials,
        timeout: Option<std::time::Duration>,
        custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        Self {
            client: super::utils::build_http_client(timeout, custom_headers),
            credentials,
            auth_cache: Mutex::new(None),
            bucket_id_cache: Mutex::new(None),
        }
    }

    /// Custom authentication endpoint (useful for mocking).
    fn auth_endpoint(&self) -> String {
        if let Some(ref ep) = self.credentials.endpoint {
            ep.trim_end_matches('/').to_string()
        } else {
            "https://api.backblazeb2.com".to_string()
        }
    }

    /// Authorizes the account against B2 and caches the tokens/URLs.
    async fn authorize(&self) -> Result<B2Auth, StorageError> {
        {
            let cache = self.auth_cache.lock().unwrap();
            if let Some(ref auth) = *cache {
                return Ok(auth.clone());
            }
        }

        let auth_url = format!("{}/b2api/v2/b2_authorize_account", self.auth_endpoint());
        let res = self.client.get(&auth_url)
            .basic_auth(&self.credentials.key_id, Some(&self.credentials.application_key))
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(translate_http_error(res, "B2", "authorize").await);
        }

        let body: B2AuthorizeResponse = res.json().await?;
        let auth = B2Auth {
            account_id: body.account_id,
            auth_token: body.authorization_token,
            api_url: body.api_url,
            download_url: body.download_url,
        };

        let mut cache = self.auth_cache.lock().unwrap();
        *cache = Some(auth.clone());
        Ok(auth)
    }

    /// Resolves the bucket ID from the bucket name and caches it.
    async fn get_bucket_id(&self, auth: &B2Auth) -> Result<String, StorageError> {
        {
            let cache = self.bucket_id_cache.lock().unwrap();
            if let Some(ref id) = *cache {
                return Ok(id.clone());
            }
        }

        let url = format!("{}/b2api/v2/b2_list_buckets", auth.api_url);
        let res = self.client.post(&url)
            .header("Authorization", &auth.auth_token)
            .json(&ListBucketsRequest {
                account_id: auth.account_id.clone(),
                bucket_name: self.credentials.bucket.clone(),
            })
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(translate_http_error(res, "B2", "list_buckets").await);
        }

        let body: ListBucketsResponse = res.json().await?;
        let bucket = body.buckets.first()
            .ok_or_else(|| StorageError::NotFound(format!("Bucket '{}' not found", self.credentials.bucket)))?;

        let mut cache = self.bucket_id_cache.lock().unwrap();
        *cache = Some(bucket.bucket_id.clone());
        Ok(bucket.bucket_id.clone())
    }

    fn format_path<'a>(&self, remote_path: &'a str) -> std::borrow::Cow<'a, str> {
        crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for B2Provider {
    fn name(&self) -> &str {
        "B2"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let clean_path = self.format_path(remote_path).into_owned();
            info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

            let auth = self.authorize().await?;
            let bucket_id = self.get_bucket_id(&auth).await?;
            let file_content = fs::read(local_path).await?;
            let len = file_content.len() as u64;

            // 1. Get Upload URL
            let get_url_endpoint = format!("{}/b2api/v2/b2_get_upload_url", auth.api_url);
            let res = self.client.post(&get_url_endpoint)
                .header("Authorization", &auth.auth_token)
                .json(&GetUploadUrlRequest { bucket_id })
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(translate_http_error(res, self.name(), "get_upload_url").await);
            }

            let upload_info: B2UploadUrlResponse = res.json().await?;

            // 2. Upload file
            let encoded_name = url_encode(&clean_path);
            let upload_res = self.client.post(&upload_info.upload_url)
                .header("Authorization", &upload_info.authorization_token)
                .header("X-Bz-File-Name", &encoded_name)
                .header("Content-Type", "b2/x-auto")
                .header("Content-Length", len)
                .header("X-Bz-Content-Sha1", "do_not_verify")
                .body(file_content)
                .send()
                .await?;

            if !upload_res.status().is_success() {
                return Err(translate_http_error(upload_res, self.name(), "upload").await);
            }

            Ok(())
        }).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "download", || async {
            let clean_path = self.format_path(remote_path).into_owned();
            let auth = self.authorize().await?;

            // B2 download URL format: {downloadUrl}/file/{bucketName}/{fileName}
            let encoded_name = url_encode(&clean_path);
            let download_url = format!("{}/file/{}/{}", auth.download_url, self.credentials.bucket, encoded_name);

            let res = self.client.get(&download_url)
                .header("Authorization", &auth.auth_token)
                .send()
                .await?;

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
            let clean_path = self.format_path(remote_path).into_owned();
            let auth = self.authorize().await?;
            let bucket_id = self.get_bucket_id(&auth).await?;

            // 1. List file names starting with this filename to find the file ID
            let list_url = format!("{}/b2api/v2/b2_list_file_names", auth.api_url);
            let list_res = self.client.post(&list_url)
                .header("Authorization", &auth.auth_token)
                .json(&ListFileNamesRequest {
                    bucket_id: bucket_id.clone(),
                    start_file_name: Some(clean_path.clone()),
                    max_file_count: Some(1),
                    prefix: None,
                })
                .send()
                .await?;

            if !list_res.status().is_success() {
                return Err(translate_http_error(list_res, self.name(), "list_for_delete").await);
            }

            let list_body: ListFileNamesResponse = list_res.json().await?;
            let file = list_body.files.first()
                .filter(|f| f.file_name == clean_path)
                .ok_or_else(|| StorageError::NotFound(format!("File '{}' not found", clean_path)))?;

            // 2. Delete the specific file version
            let delete_url = format!("{}/b2api/v2/b2_delete_file_version", auth.api_url);
            let del_res = self.client.post(&delete_url)
                .header("Authorization", &auth.auth_token)
                .json(&DeleteFileVersionRequest {
                    file_name: file.file_name.clone(),
                    file_id: file.file_id.clone(),
                })
                .send()
                .await?;

            if !del_res.status().is_success() {
                return Err(translate_http_error(del_res, self.name(), "delete").await);
            }

            Ok(())
        }).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let clean_path = self.format_path(remote_path).into_owned();
            let auth = self.authorize().await?;
            let bucket_id = self.get_bucket_id(&auth).await?;

            let prefix = if clean_path.is_empty() {
                None
            } else {
                if clean_path.ends_with('/') {
                    Some(clean_path.clone())
                } else {
                    Some(format!("{}/", clean_path))
                }
            };

            let list_url = format!("{}/b2api/v2/b2_list_file_names", auth.api_url);
            let mut items = Vec::new();
            let mut start_file_name: Option<String> = None;

            loop {
                let res = self.client.post(&list_url)
                    .header("Authorization", &auth.auth_token)
                    .json(&ListFileNamesRequest {
                        bucket_id: bucket_id.clone(),
                        start_file_name: start_file_name.clone(),
                        max_file_count: None,
                        prefix: prefix.clone(),
                    })
                    .send()
                    .await?;

                if !res.status().is_success() {
                    return Err(translate_http_error(res, self.name(), "list").await);
                }

                let body: ListFileNamesResponse = res.json().await?;
                for file in body.files {
                    let mut item_path = PathBuf::from(&file.file_name);
                    if let Some(ref dest_folder) = self.credentials.common.destination_folder {
                        let clean_dest = dest_folder.trim_matches('/');
                        if !clean_dest.is_empty() {
                            if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
                                item_path = stripped.to_path_buf();
                            }
                        }
                    }

                    // Convert B2 timestamp (milliseconds since epoch) to SystemTime
                    let modified = SystemTime::UNIX_EPOCH + Duration::from_millis(file.upload_timestamp);

                    items.push(StorageItem {
                        path: item_path,
                        size: file.content_length,
                        modified,
                        is_dir: false, // B2 is a flat namespace, folders are virtual
                        checksum: None,
                        permissions: None,
                });
                }

                if body.next_file_name.is_some() {
                    start_file_name = body.next_file_name;
                } else {
                    break;
                }
            }

            Ok(items)
        }).await
    }

}


/// Builder for [`B2Provider`].
pub struct B2ProviderBuilder {
    pub credentials: B2Credentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl B2ProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: B2Credentials) -> Self {
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
    pub fn build(self) -> B2Provider {
        B2Provider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}
