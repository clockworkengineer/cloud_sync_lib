//! IPFS/Pinning Service (Pinata) storage backend provider implementation.
//!
//! Handles interaction with the Pinata Pinning API. Supports file upload (pinning),
//! gateway-based file download, unpinning on deletion, and pinList querying.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::IPFSCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tracing::info;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct PinataMetadata {
    name: String,
}

#[derive(Deserialize, Debug)]
struct PinataRow {
    ipfs_pin_hash: String,
    metadata: PinataMetadata,
    size: u64,
    date_pinned: Option<String>,
}

#[derive(Deserialize, Debug)]
struct PinataPinListResponse {
    rows: Vec<PinataRow>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct PinataPinFileResponse {
    #[serde(rename = "IpfsHash")]
    ipfs_hash: String,
}

/// Storage provider client for IPFS Pinning Service (Pinata) REST API.
pub struct IPFSProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: IPFSCredentials,
    /// Pinning API base URL.
    api_url: String,
    /// Gateway URL for downloads.
    gateway_url: String,
}

impl IPFSProvider {
    /// Creates a new `IPFSProvider` using the provided credentials.
    pub fn new(credentials: IPFSCredentials) -> Self {
        let api_url = if let Some(ref ep) = credentials.endpoint {
            ep.trim_end_matches('/').to_string()
        } else {
            "https://api.pinata.cloud".to_string()
        };

        let gateway_url = if let Some(ref gw) = credentials.gateway_url {
            gw.clone()
        } else {
            "https://gateway.pinata.cloud/ipfs/".to_string()
        };

        Self {
            client: reqwest::Client::new(),
            credentials,
            api_url,
            gateway_url,
        }
    }

    fn format_path(&self, remote_path: &str) -> String {
        crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }

    /// Helper to query the Pinata PinList to find the CID (IpfsHash) for a given remote filename.
    async fn resolve_cid(&self, remote_path: &str) -> Result<String, StorageError> {
        let query_url = format!("{}/data/pinList", self.api_url);
        let res = self.client.get(&query_url)
            .bearer_auth(&self.credentials.jwt_token)
            .query(&[
                ("status", "pinned"),
                ("metadata[name]", remote_path),
            ])
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "resolve_cid").await);
        }

        let body: PinataPinListResponse = res.json().await?;
        let row = body.rows.first()
            .ok_or_else(|| StorageError::NotFound(format!("File '{}' not found in IPFS pinned index", remote_path)))?;

        Ok(row.ipfs_pin_hash.clone())
    }
}

#[async_trait]
impl StorageBackend for IPFSProvider {
    fn name(&self) -> &str {
        "IPFS"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);
        info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

        let file_content = fs::read(local_path).await?;
        let file_name = Path::new(&clean_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&clean_path)
            .to_string();

        let upload_url = format!("{}/pinning/pinFileToIPFS", self.api_url);

        // Build pinataMetadata JSON string
        let metadata_json = serde_json::json!({
            "name": clean_path
        }).to_string();

        // Build multipart body
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(file_content).file_name(file_name))
            .text("pinataMetadata", metadata_json);

        let res = self.client.post(&upload_url)
            .bearer_auth(&self.credentials.jwt_token)
            .multipart(form)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "upload").await);
        }

        let _body: PinataPinFileResponse = res.json().await?;
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);
        let cid = self.resolve_cid(&clean_path).await?;

        let download_url = if self.gateway_url.ends_with('/') {
            format!("{}{}", self.gateway_url, cid)
        } else {
            format!("{}/{}", self.gateway_url, cid)
        };

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
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);
        let cid = self.resolve_cid(&clean_path).await?;

        let unpin_url = format!("{}/pinning/unpin/{}", self.api_url, cid);
        let res = self.client.delete(&unpin_url)
            .bearer_auth(&self.credentials.jwt_token)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "delete").await);
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let clean_path = self.format_path(remote_path);
        let list_url = format!("{}/data/pinList", self.api_url);

        let mut req = self.client.get(&list_url)
            .bearer_auth(&self.credentials.jwt_token)
            .query(&[("status", "pinned")]);

        if !clean_path.is_empty() {
            req = req.query(&[("metadata[name]", &clean_path)]);
        }

        let res = req.send().await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "list").await);
        }

        let list_response: PinataPinListResponse = res.json().await?;
        let mut items = Vec::new();

        for row in list_response.rows {
            let mut item_path = PathBuf::from(&row.metadata.name);
            if let Some(ref dest_folder) = self.credentials.common.destination_folder {
                let clean_dest = dest_folder.trim_matches('/');
                if !clean_dest.is_empty() {
                    if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
                        item_path = stripped.to_path_buf();
                    }
                }
            }

            let modified = row.date_pinned
                .as_ref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map(SystemTime::from)
                .unwrap_or(SystemTime::now());

            items.push(StorageItem {
                path: item_path,
                size: row.size,
                modified,
                is_dir: false,
            });
        }

        Ok(items)
    }

    fn sync_mode(&self) -> super::SyncMode {
        use super::ProviderConfig;
        self.credentials.sync_mode()
    }
}
