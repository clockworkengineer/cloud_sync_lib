//! Google Drive storage backend provider implementation.
//!
//! Handles interaction with the Google Drive API v3. Supports full OAuth2-based
//! upload, download, delete, and list operations, with recursive directory resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::local_sim::LocalSimulation;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Storage provider client for Google Drive.
///
/// If credentials are provided, connects to the Google Drive v3 REST API.
/// If credentials are `None`, simulates behavior by reading/writing files
/// inside the local directory specified by `root_dir`.
pub struct GoogleDriveProvider {
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
    auth_url: String,
    api_url: String,
    upload_url: String,
    local_sim: LocalSimulation,
}

impl GoogleDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        let local_sim = LocalSimulation::new(root_dir, "Google Drive".to_string());
        Ok(Self {
            client: reqwest::Client::new(),
            credentials,
            auth_url: "https://oauth2.googleapis.com/token".to_string(),
            api_url: "https://www.googleapis.com/drive/v3/files".to_string(),
            upload_url: "https://www.googleapis.com/upload/drive/v3/files".to_string(),
            local_sim,
        })
    }

    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, upload_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.upload_url = upload_url;
        self
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No Google Drive credentials configured".into())
        })?;

        refresh_oauth2_token(
            &self.client,
            &self.auth_url,
            &creds.client_id,
            &creds.client_secret,
            &creds.refresh_token,
            self.name(),
        ).await
    }

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

    // Resolves a path (e.g. "a/b/c.txt") to a Google Drive file ID.
    // If create_parents is true, it will create any missing folders in the hierarchy.
    async fn get_or_create_file_id(&self, token: &str, path: &str, is_folder: bool) -> Result<String, StorageError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut parent_id = "root".to_string();

        if let Some(ref creds) = self.credentials {
            if let Some(ref dest_folder) = creds.destination_folder {
                if !dest_folder.is_empty() {
                    parent_id = self.get_or_create_folder_id(token, &parent_id, dest_folder).await?;
                }
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

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            return self.local_sim.upload(local_path, remote_path).await;
        }

        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        info!("[{}] Real upload starting for '{}' (ID: {})", self.name(), remote_path, file_id);
        let file_content = fs::read(local_path).await?;
        
        let upload_url = format!("{}/{}?uploadType=media", self.upload_url, file_id);
        let res = self.client.patch(&upload_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "upload").await);
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            return self.local_sim.download(remote_path, local_path).await;
        }

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

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let bytes = res.bytes().await?;
        fs::write(local_path, bytes).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            return self.local_sim.delete(remote_path).await;
        }

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
        if self.credentials.is_none() {
            return self.local_sim.list(remote_path).await;
        }

        let token = self.get_access_token().await?;
        let folder_id = self.get_or_create_file_id(&token, remote_path, true).await?;

        let query = format!("'{}' in parents and trashed = false", folder_id);
        let res = self.client.get(&self.api_url)
            .bearer_auth(&token)
            .query(&[("q", &query), ("fields", &"files(id, name, size, mimeType, modifiedTime)".to_string())])
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
}
