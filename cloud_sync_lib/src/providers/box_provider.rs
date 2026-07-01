//! Box storage backend provider implementation.
//!
//! Handles interaction with the Box v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;
use serde::Deserialize;

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
}

#[derive(Deserialize, Debug)]
struct BoxItem {
    id: String,
    name: String,
    #[serde(rename = "type")]
    item_type: String,
    size: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct BoxFolderItems {
    entries: Vec<BoxItem>,
}

impl BoxProvider {
    /// Creates a new `BoxProvider` using the provided OAuth credentials.
    pub fn new(credentials: OAuthCredentials) -> Self {
        Self {
            client: reqwest::Client::new(),
            credentials,
            auth_url: "https://api.box.com/oauth2/token".to_string(),
            api_url: "https://api.box.com/2.0".to_string(),
            upload_url: "https://upload.box.com/api/2.0".to_string(),
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

    /// Helper to retrieve a valid OAuth access token, refreshing it if necessary.
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

    /// Resolves the full path to Box item (ID and type) by traversing from the root folder ("0").
    ///
    /// If `create_folders` is true, missing intermediate directories will be automatically created.
    async fn resolve_path(&self, token: &str, path: &str, create_folders: bool) -> Result<(String, bool), StorageError> {
        // Resolve target path incorporating destination_folder prefix
        let clean_path = path.trim_start_matches('/');
        let mut segments = Vec::new();

        if let Some(ref dest) = self.credentials.destination_folder {
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
                return Err(StorageError::Provider(format!(
                    "Path resolution error: intermediate segment '{}' is not a folder", segment
                )));
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
                    if create_folders && (!is_last || (is_last && is_dir)) {
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
            StorageError::Provider("Invalid file name".to_string())
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
        let token = self.get_access_token().await?;
        let (folder_id, is_dir) = self.resolve_path(&token, path, false).await?;

        if !is_dir {
            return Err(StorageError::Provider("Target path is not a folder".to_string()));
        }

        let url = format!("{}/folders/{}/items", self.api_url, folder_id);
        let res = self.client.get(&url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "list").await);
        }

        let items = res.json::<BoxFolderItems>().await?;
        let storage_items = items.entries.into_iter().map(|item| {
            StorageItem {
                path: PathBuf::from(item.name),
                is_dir: item.item_type == "folder",
                size: item.size.unwrap_or(0),
                modified: std::time::SystemTime::now(),
            }
        }).collect();

        Ok(storage_items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
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
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let token = self.get_access_token().await?;
        let (file_id, is_dir) = self.resolve_path(&token, remote_path, false).await?;

        if is_dir {
            return Err(StorageError::Provider("Cannot download a directory".to_string()));
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
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
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
    }
}
