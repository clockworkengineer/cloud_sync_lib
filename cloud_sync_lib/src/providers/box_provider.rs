//! Box storage backend provider implementation.
//!
//! Handles interaction with the Box v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;
use serde::Deserialize;

use std::sync::Mutex;
use std::time::{Instant, Duration};

struct CachedToken {
    access_token: String,
    refresh_token: String,
    expires_at: Instant,
}

/// Storage provider client for Box REST API.
pub struct BoxProvider {
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
    /// Token cache.
    token_cache: Mutex<Option<CachedToken>>,
}

#[derive(Deserialize, Debug)]
struct BoxItem {
    id: String,
    name: String,
    #[serde(rename = "type")]
    item_type: String,
    size: Option<u64>,
    content_modified_at: Option<String>,
    sha1: Option<String>,
}

#[derive(Deserialize, Debug)]
struct BoxFolderItems {
    entries: Vec<BoxItem>,
}

#[derive(Deserialize, Debug)]
struct BoxTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

impl BoxProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: OAuthCredentials) -> BoxProviderBuilder {
        BoxProviderBuilder::new(credentials)
    }

    /// Creates a new `BoxProvider` using the provided OAuth credentials.
    pub fn new(credentials: OAuthCredentials) -> Self {
        Self {
            client: super::utils::build_http_client(),
            credentials,
            auth_url: "https://api.box.com/oauth2/token".to_string(),
            api_url: "https://api.box.com/2.0".to_string(),
            upload_url: "https://upload.box.com/api/2.0".to_string(),
            token_cache: Mutex::new(None),
        }
    }

    /// Configures custom endpoints, useful for mocking during tests.
    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, upload_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.upload_url = upload_url;
        self
    }

    /// Helper to update local config files when the Box refresh token rotates.
    fn update_config_files(&self, new_refresh_token: &str) {
        for filename in &["config.toml", "private_config.toml"] {
            if let Ok(content) = std::fs::read_to_string(filename) {
                if let Some(box_idx) = content.find("[box_credentials]") {
                    let suffix = &content[box_idx..];
                    if let Some(token_idx) = suffix.find("refresh_token") {
                        if let Some(start_quote) = suffix[token_idx..].find('"') {
                            let absolute_start = box_idx + token_idx + start_quote + 1;
                            let remainder = &suffix[token_idx + start_quote + 1..];
                            if let Some(end_quote) = remainder.find('"') {
                                let absolute_end = absolute_start + end_quote;
                                let mut new_content = content[..absolute_start].to_string();
                                new_content.push_str(new_refresh_token);
                                new_content.push_str(&content[absolute_end..]);
                                let _ = std::fs::write(filename, new_content);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Helper to retrieve a valid OAuth access token, refreshing it if necessary.
    async fn get_access_token(&self) -> Result<String, StorageError> {
        let (active_refresh_token, needs_refresh) = {
            let cache = self.token_cache.lock().unwrap();
            if let Some(ref cached) = *cache {
                // If token is still valid for at least 60 seconds, use it
                if cached.expires_at > Instant::now() + Duration::from_secs(60) {
                    return Ok(cached.access_token.clone());
                }
                (cached.refresh_token.clone(), true)
            } else {
                (self.credentials.refresh_token.clone(), true)
            }
        };

        if !needs_refresh {
            // Unreachable but satisfies Rust
            return Err(StorageError::Authentication("Token fetch logic error".to_string()));
        }

        // Perform OAuth refresh
        let params = [
            ("client_id", self.credentials.client_id.as_str()),
            ("client_secret", self.credentials.client_secret.as_str()),
            ("refresh_token", active_refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let res = self.client.post(&self.auth_url)
            .form(&params)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "refresh_token").await);
        }

        let token_resp = res.json::<BoxTokenResponse>().await?;
        
        // Update local configs with the rotated refresh token
        self.update_config_files(&token_resp.refresh_token);

        let mut cache = self.token_cache.lock().unwrap();
        *cache = Some(CachedToken {
            access_token: token_resp.access_token.clone(),
            refresh_token: token_resp.refresh_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(token_resp.expires_in),
        });

        Ok(token_resp.access_token)
    }

    /// Resolves the full path to Box item (ID and type) by traversing from the root folder ("0").
    ///
    /// If `create_folders` is true, missing intermediate directories will be automatically created.
    async fn resolve_path(&self, token: &str, path: &str, create_folders: bool) -> Result<(String, bool), StorageError> {
        let normalized_dest_opt = self.credentials.common.destination_folder.as_ref().map(|dest| super::utils::normalize_remote_path(dest));
        let normalized_path = super::utils::normalize_remote_path(path);
        let clean_path = normalized_path.trim_start_matches('/');
        let mut segments = Vec::new();

        if let Some(ref dest) = normalized_dest_opt {
            let clean_dest = dest.trim_matches('/');
            if !clean_dest.is_empty() {
                for seg in clean_dest.split('/') {
                    segments.push(seg);
                }
            }
        }

        if !clean_path.is_empty() {
            for seg in clean_path.split('/') {
                segments.push(seg);
            }
        }

        let mut current_id = "0".to_string();
        let mut is_dir = true;

        for (i, segment) in segments.iter().enumerate() {
            if segment.is_empty() {
                continue;
            }
            if !is_dir {
                return Err(StorageError::Provider { message: format!(
                    "Path resolution error: intermediate segment '{}' is not a folder", segment
                ), status: None });
            }

            // List current folder
            let url = format!("{}/folders/{}/items", self.api_url, current_id);
            let res = self.client.get(&url)
                .bearer_auth(token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "list_folder_items").await);
            }

            let folder_items = res.json::<BoxFolderItems>().await?;
            let found = folder_items.entries.into_iter().find(|item| item.name == **segment);

            match found {
                Some(item) => {
                    current_id = item.id;
                    is_dir = item.item_type == "folder";
                }
                None => {
                    // Item not found. If it's not the last segment, or it's a directory segment we want to create:
                    let is_last = i == segments.len() - 1;
                    if create_folders && (!is_last || is_dir) {
                        // Create folder
                        let create_url = format!("{}/folders", self.api_url);
                        let body = serde_json::json!({
                            "name": segment,
                            "parent": { "id": current_id }
                        });
                        let create_res = self.client.post(&create_url)
                            .bearer_auth(token)
                            .json(&body)
                            .send()
                            .await?;

                        if !create_res.status().is_success() {
                            return Err(parse_response_error(create_res, self.name(), "create_folder").await);
                        }

                        let new_folder = create_res.json::<BoxItem>().await?;
                        current_id = new_folder.id;
                        is_dir = true;
                    } else {
                        return Err(StorageError::NotFound(format!("Path segment '{}' not found", segment)));
                    }
                }
            }
        }

        Ok((current_id, is_dir))
    }

    /// Resolves the parent folder ID and the filename for a given remote path.
    async fn resolve_parent_and_name(&self, token: &str, remote_path: &str) -> Result<(String, String), StorageError> {
        let path = Path::new(remote_path);
        let parent_path = path.parent().and_then(|p| p.to_str()).unwrap_or("");
        let file_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
            StorageError::Provider { message: "Invalid file name".to_string(), status: None }
        })?;

        let (parent_id, _) = self.resolve_path(token, parent_path, true).await?;
        Ok((parent_id, file_name.to_string()))
    }
}

#[async_trait]
impl StorageBackend for BoxProvider {
    fn name(&self) -> &'static str {
        "Box"
    }

    async fn list(&self, path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let token = self.get_access_token().await?;
            let (folder_id, is_dir) = self.resolve_path(&token, path, false).await?;

            if !is_dir {
                return Err(StorageError::Provider { message: "Target path is not a folder".to_string(), status: None });
            }

            let url = format!("{}/folders/{}/items?fields=id,type,name,size,content_modified_at,sha1", self.api_url, folder_id);
            let res = self.client.get(&url)
                .bearer_auth(&token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "list").await);
            }

            let items = res.json::<BoxFolderItems>().await?;
            let storage_items = items.entries.into_iter().map(|item| {
                let modified = item.content_modified_at
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                    .map(|dt| std::time::SystemTime::from(dt))
                    .unwrap_or_else(std::time::SystemTime::now);

                let name = item.name;
                let rel_path = if path.is_empty() {
                    name
                } else {
                    format!("{}/{}", path, name)
                };

                StorageItem {
                    path: PathBuf::from(rel_path),
                    is_dir: item.item_type == "folder",
                    size: item.size.unwrap_or(0),
                    modified,
                    checksum: item.sha1,
                }
            }).collect();

            Ok(storage_items)
        }).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "create_folder", || async {
            let token = self.get_access_token().await?;
            let _ = self.resolve_path(&token, remote_path, true).await?;
            Ok(())
        }).await
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let token = self.get_access_token().await?;
            let (parent_id, file_name) = self.resolve_parent_and_name(&token, remote_path).await?;

            // Check if file already exists in parent folder
            let url = format!("{}/folders/{}/items", self.api_url, parent_id);
            let res = self.client.get(&url)
                .bearer_auth(&token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "check_existing_file").await);
            }

            let folder_items = res.json::<BoxFolderItems>().await?;
            let existing_file = folder_items.entries.into_iter()
                .find(|item| item.name == file_name && item.item_type == "file");

            let file_bytes = fs::read(local_path).await?;
            let file_part = reqwest::multipart::Part::bytes(file_bytes)
                .file_name(file_name.clone());

            match existing_file {
                Some(file) => {
                    // File exists: Upload new version (overwrite)
                    info!("[Box] Real upload starting (version update) for file ID: {}", file.id);
                    let upload_url = format!("{}/files/{}/content", self.upload_url, file.id);
                    let form = reqwest::multipart::Form::new().part("file", file_part);

                    let upload_res = self.client.post(&upload_url)
                        .bearer_auth(&token)
                        .multipart(form)
                        .send()
                        .await?;

                    if !upload_res.status().is_success() {
                        return Err(parse_response_error(upload_res, self.name(), "upload_version").await);
                    }
                }
                None => {
                    // File does not exist: Upload new file
                    info!("[Box] Real upload starting (new file) for '{}'", file_name);
                    let upload_url = format!("{}/files/content", self.upload_url);
                    let attributes = serde_json::json!({
                        "name": file_name,
                        "parent": { "id": parent_id }
                    }).to_string();

                    let form = reqwest::multipart::Form::new()
                        .text("attributes", attributes)
                        .part("file", file_part);

                    let upload_res = self.client.post(&upload_url)
                        .bearer_auth(&token)
                        .multipart(form)
                        .send()
                        .await?;

                    if !upload_res.status().is_success() {
                        return Err(parse_response_error(upload_res, self.name(), "upload_new").await);
                    }
                }
            }

            Ok(())
        }).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "download", || async {
            let token = self.get_access_token().await?;
            let (file_id, is_dir) = self.resolve_path(&token, remote_path, false).await?;

            if is_dir {
                return Err(StorageError::Provider { message: "Cannot download a directory".to_string(), status: None });
            }

            let url = format!("{}/files/{}/content", self.api_url, file_id);
            let res = self.client.get(&url)
                .bearer_auth(&token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "download").await);
            }

            let bytes = res.bytes().await?;
            fs::write(local_path, bytes).await?;
            Ok(())
        }).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "delete", || async {
            let token = self.get_access_token().await?;
            let (item_id, is_dir) = self.resolve_path(&token, remote_path, false).await?;

            let url = if is_dir {
                format!("{}/folders/{}", self.api_url, item_id)
            } else {
                format!("{}/files/{}", self.api_url, item_id)
            };

            let res = self.client.delete(&url)
                .bearer_auth(&token)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "delete").await);
            }

            Ok(())
        }).await
    }

    async fn compute_local_checksum(&self, local_path: &Path) -> Result<Option<String>, StorageError> {
        Ok(crate::checksum::compute_sha1(local_path).await.ok())
    }
}



/// Builder for [`BoxProvider`].
pub struct BoxProviderBuilder {
    pub credentials: OAuthCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl BoxProviderBuilder {
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
    pub fn build(self) -> BoxProvider {
        BoxProvider::new(self.credentials)
    }
}
