//! Dropbox storage backend provider implementation.
//!
//! Handles interaction with the Dropbox v2 REST API. Supports full OAuth2-based
//! upload, download, delete, and list operations, with custom prefix path resolution.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::OAuthCredentials;
use crate::providers::local_sim::LocalSimulation;
use crate::providers::utils::{refresh_oauth2_token, parse_response_error};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Storage provider client for Dropbox.
///
/// If credentials are provided, connects to the Dropbox v2 API endpoints.
/// If credentials are `None`, simulates behavior by reading/writing files
/// inside the local directory specified by `root_dir`.
pub struct DropboxProvider {
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
    auth_url: String,
    api_url: String,
    content_url: String,
    local_sim: LocalSimulation,
}

impl DropboxProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        let local_sim = LocalSimulation::new(root_dir, "Dropbox".to_string());
        Ok(Self {
            client: reqwest::Client::new(),
            credentials,
            auth_url: "https://api.dropbox.com/oauth2/token".to_string(),
            api_url: "https://api.dropboxapi.com/2/files".to_string(),
            content_url: "https://content.dropboxapi.com/2/files".to_string(),
            local_sim,
        })
    }

    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, content_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.content_url = content_url;
        self
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No Dropbox credentials configured".into())
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

    fn format_path(&self, path: &str) -> String {
        let clean_path = path.trim_start_matches('/');
        let mut full_path = String::new();

        if let Some(ref creds) = self.credentials {
            if let Some(ref dest_folder) = creds.destination_folder {
                let clean_dest = dest_folder.trim_matches('/');
                if !clean_dest.is_empty() {
                    full_path.push_str("/");
                    full_path.push_str(clean_dest);
                }
            }
        }

        if !clean_path.is_empty() {
            full_path.push_str("/");
            full_path.push_str(clean_path);
        }

        full_path
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            return self.local_sim.upload(local_path, remote_path).await;
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        info!("[{}] Real upload starting for '{}'", self.name(), dbx_path);
        let file_content = fs::read(local_path).await?;

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
        if self.credentials.is_none() {
            return self.local_sim.list(remote_path).await;
        }

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
}
