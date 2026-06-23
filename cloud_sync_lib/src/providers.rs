use crate::traits::{StorageBackend, StorageError, StorageItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub destination_folder: Option<String>,
}

/// A Google Drive storage provider that can sync either to a real API (if credentials are set)
/// or fall back to a local folder simulation.
pub struct GoogleDriveProvider {
    root_dir: PathBuf,
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
    auth_url: String,
    api_url: String,
    upload_url: String,
}

impl GoogleDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self {
            root_dir,
            client: reqwest::Client::new(),
            credentials,
            auth_url: "https://oauth2.googleapis.com/token".to_string(),
            api_url: "https://www.googleapis.com/drive/v3/files".to_string(),
            upload_url: "https://www.googleapis.com/upload/drive/v3/files".to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_endpoints(mut self, auth_url: String, api_url: String, upload_url: String) -> Self {
        self.auth_url = auth_url;
        self.api_url = api_url;
        self.upload_url = upload_url;
        self
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No Google Drive credentials configured".into())
        })?;

        let params = [
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", &"refresh_token".to_string()),
        ];

        let res = self.client.post(&self.auth_url)
            .form(&params)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let token = res["access_token"].as_str().ok_or_else(|| {
            StorageError::Authentication(format!("Failed to retrieve access token: {:?}", res))
        })?;

        Ok(token.to_string())
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
            // Local fallback simulation
            let destination = self.resolve(remote_path);
            info!("[{}] (Simulated) Uploading local file {:?} to remote path '{}'", self.name(), local_path, remote_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(local_path, &destination).await?;
            return Ok(());
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
            return Err(StorageError::Provider(format!("Failed to upload file content to Google Drive: {}", res.status())));
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let source = self.resolve(remote_path);
            info!("[{}] (Simulated) Downloading remote path '{}' to local file {:?}", self.name(), remote_path, local_path);
            if !source.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source, local_path).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        let download_url = format!("{}/{}?alt=media", self.api_url, file_id);
        let res = self.client.get(&download_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to download file from Google Drive: {}", res.status())));
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
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Deleting remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if target.is_dir() {
                fs::remove_dir_all(&target).await?;
            } else {
                fs::remove_file(&target).await?;
            }
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let file_id = self.get_or_create_file_id(&token, remote_path, false).await?;

        let delete_url = format!("{}/{}", self.api_url, file_id);
        let res = self.client.delete(&delete_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to delete file on Google Drive: {}", res.status())));
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if self.credentials.is_none() {
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Listing contents of remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            let mut items = Vec::new();
            let mut entries = fs::read_dir(&target).await?;
            while let Some(entry) = entries.next_entry().await? {
                let metadata = entry.metadata().await?;
                items.push(StorageItem {
                    path: entry.path().strip_prefix(&self.root_dir).unwrap_or(&entry.path()).to_path_buf(),
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(std::time::SystemTime::now()),
                    is_dir: metadata.is_dir(),
                });
            }
            return Ok(items);
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

/// A Dropbox storage provider that can sync either to a real API (if credentials are set)
/// or fall back to a local folder simulation.
pub struct DropboxProvider {
    root_dir: PathBuf,
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
}

impl DropboxProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self {
            root_dir,
            client: reqwest::Client::new(),
            credentials,
        })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No Dropbox credentials configured".into())
        })?;

        let params = [
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", &"refresh_token".to_string()),
        ];

        let res = self.client.post("https://api.dropbox.com/oauth2/token")
            .form(&params)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let token = res["access_token"].as_str().ok_or_else(|| {
            StorageError::Authentication(format!("Failed to retrieve Dropbox access token: {:?}", res))
        })?;

        Ok(token.to_string())
    }

    fn format_path(&self, path: &str) -> String {
        let formatted = path.trim_start_matches('/');
        if formatted.is_empty() {
            "".to_string()
        } else {
            format!("/{}", formatted)
        }
    }
}

#[async_trait]
impl StorageBackend for DropboxProvider {
    fn name(&self) -> &str {
        "Dropbox"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let destination = self.resolve(remote_path);
            info!("[{}] (Simulated) Uploading local file {:?} to remote path '{}'", self.name(), local_path, remote_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(local_path, &destination).await?;
            return Ok(());
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

        let res = self.client.post("https://content.dropboxapi.com/2/files/upload")
            .bearer_auth(&token)
            .header("Dropbox-API-Arg", serde_json::to_string(&api_arg).unwrap())
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(StorageError::Provider(format!("Failed to upload to Dropbox: {}", body)));
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let source = self.resolve(remote_path);
            info!("[{}] (Simulated) Downloading remote path '{}' to local file {:?}", self.name(), remote_path, local_path);
            if !source.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source, local_path).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let api_arg = serde_json::json!({
            "path": dbx_path
        });

        let res = self.client.post("https://content.dropboxapi.com/2/files/download")
            .bearer_auth(&token)
            .header("Dropbox-API-Arg", serde_json::to_string(&api_arg).unwrap())
            .header("Content-Type", "")
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to download from Dropbox: {}", res.status())));
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
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Deleting remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if target.is_dir() {
                fs::remove_dir_all(&target).await?;
            } else {
                fs::remove_file(&target).await?;
            }
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let body = serde_json::json!({
            "path": dbx_path
        });

        let res = self.client.post("https://api.dropboxapi.com/2/files/delete_v2")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to delete on Dropbox: {}", res.status())));
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if self.credentials.is_none() {
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Listing contents of remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            let mut items = Vec::new();
            let mut entries = fs::read_dir(&target).await?;
            while let Some(entry) = entries.next_entry().await? {
                let metadata = entry.metadata().await?;
                items.push(StorageItem {
                    path: entry.path().strip_prefix(&self.root_dir).unwrap_or(&entry.path()).to_path_buf(),
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(std::time::SystemTime::now()),
                    is_dir: metadata.is_dir(),
                });
            }
            return Ok(items);
        }

        let token = self.get_access_token().await?;
        let dbx_path = self.format_path(remote_path);

        let body = serde_json::json!({
            "path": dbx_path,
            "recursive": false
        });

        let res = self.client.post("https://api.dropboxapi.com/2/files/list_folder")
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

/// A OneDrive storage provider that can sync either to a real API (if credentials are set)
/// or fall back to a local folder simulation.
pub struct OneDriveProvider {
    root_dir: PathBuf,
    client: reqwest::Client,
    credentials: Option<OAuthCredentials>,
}

impl OneDriveProvider {
    pub async fn new(root_dir: impl Into<PathBuf>, credentials: Option<OAuthCredentials>) -> Result<Self, std::io::Error> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir).await?;
        Ok(Self {
            root_dir,
            client: reqwest::Client::new(),
            credentials,
        })
    }

    fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    async fn get_access_token(&self) -> Result<String, StorageError> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            StorageError::Authentication("No OneDrive credentials configured".into())
        })?;

        let params = [
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("refresh_token", &creds.refresh_token),
            ("grant_type", &"refresh_token".to_string()),
        ];

        let res = self.client.post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
            .form(&params)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let token = res["access_token"].as_str().ok_or_else(|| {
            StorageError::Authentication(format!("Failed to retrieve OneDrive access token: {:?}", res))
        })?;

        Ok(token.to_string())
    }
}

#[async_trait]
impl StorageBackend for OneDriveProvider {
    fn name(&self) -> &str {
        "OneDrive"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let destination = self.resolve(remote_path);
            info!("[{}] (Simulated) Uploading local file {:?} to remote path '{}'", self.name(), local_path, remote_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(local_path, &destination).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        info!("[{}] Real upload starting for '{}'", self.name(), clean_path);
        let file_content = fs::read(local_path).await?;

        let upload_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content", clean_path);
        let res = self.client.put(&upload_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(StorageError::Provider(format!("Failed to upload to OneDrive: {}", body)));
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if self.credentials.is_none() {
            let source = self.resolve(remote_path);
            info!("[{}] (Simulated) Downloading remote path '{}' to local file {:?}", self.name(), remote_path, local_path);
            if !source.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source, local_path).await?;
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        let download_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/content", clean_path);
        let res = self.client.get(&download_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to download from OneDrive: {}", res.status())));
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
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Deleting remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            if target.is_dir() {
                fs::remove_dir_all(&target).await?;
            } else {
                fs::remove_file(&target).await?;
            }
            return Ok(());
        }

        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        let delete_url = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}", clean_path);
        let res = self.client.delete(&delete_url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(StorageError::Provider(format!("Failed to delete on OneDrive: {}", res.status())));
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if self.credentials.is_none() {
            let target = self.resolve(remote_path);
            info!("[{}] (Simulated) Listing contents of remote path '{}'", self.name(), remote_path);
            if !target.exists() {
                return Err(StorageError::NotFound(remote_path.to_string()));
            }
            let mut items = Vec::new();
            let mut entries = fs::read_dir(&target).await?;
            while let Some(entry) = entries.next_entry().await? {
                let metadata = entry.metadata().await?;
                items.push(StorageItem {
                    path: entry.path().strip_prefix(&self.root_dir).unwrap_or(&entry.path()).to_path_buf(),
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(std::time::SystemTime::now()),
                    is_dir: metadata.is_dir(),
                });
            }
            return Ok(items);
        }

        let token = self.get_access_token().await?;
        let clean_path = remote_path.trim_start_matches('/');

        let list_url = if clean_path.is_empty() {
            "https://graph.microsoft.com/v1.0/me/drive/root/children".to_string()
        } else {
            format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/children", clean_path)
        };

        let res = self.client.get(&list_url)
            .bearer_auth(&token)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let mut items = Vec::new();
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
                });
            }
        }

        Ok(items)
    }
}
