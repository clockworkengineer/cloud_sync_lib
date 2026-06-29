//! WebDAV storage backend provider implementation.
//!
//! Handles interaction with WebDAV servers (such as Nextcloud, ownCloud, or NAS systems)
//! using standard HTTP Basic Authentication and standard WebDAV HTTP verbs.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::WebDAVCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;
use quick_xml::events::Event;
use quick_xml::Reader;

/// Storage provider client for WebDAV storage servers.
pub struct WebDAVProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration (server URL, username, password).
    credentials: WebDAVCredentials,
    /// Active base URL of the WebDAV server.
    url: String,
}

impl WebDAVProvider {
    /// Creates a new `WebDAVProvider` using the provided WebDAV credentials.
    ///
    /// # Arguments
    /// * `credentials` - WebDAV credentials and server configuration.
    ///
    /// # Returns
    /// A new instance of `WebDAVProvider`.
    pub fn new(credentials: WebDAVCredentials) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: credentials.url.clone(),
            credentials,
        }
    }

    /// Configures custom endpoints, useful for mocking during tests.
    ///
    /// # Arguments
    /// * `url` - Custom WebDAV server URL.
    ///
    /// # Returns
    /// The modified `WebDAVProvider` instance.
    #[cfg(test)]
    pub fn with_endpoints(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    /// Formats the remote path, incorporating the optional destination folder prefix.
    ///
    /// # Arguments
    /// * `remote_path` - The relative destination path.
    ///
    /// # Returns
    /// The fully-resolved WebDAV absolute path string (prefixed with `/`).
    fn format_path(&self, remote_path: &str) -> String {
        let clean_path = remote_path.trim_start_matches('/');
        if let Some(ref dest_folder) = self.credentials.destination_folder {
            let clean_dest = dest_folder.trim_matches('/');
            if !clean_dest.is_empty() {
                if clean_path.is_empty() {
                    return format!("/{}", clean_dest);
                } else {
                    return format!("/{}/{}", clean_dest, clean_path);
                }
            }
        }
        if clean_path.is_empty() {
            "".to_string()
        } else {
            format!("/{}", clean_path)
        }
    }

    /// Ensures that parent directories exist on the WebDAV server for a given remote path.
    ///
    /// Performs MKCOL requests sequentially down the directory tree.
    ///
    /// # Arguments
    /// * `remote_path` - The destination path of the file we want to upload.
    ///
    /// # Returns
    /// An empty `Result`, or a `StorageError` if directory creation fails.
    async fn ensure_parent_dirs(&self, remote_path: &str) -> Result<(), StorageError> {
        let parts: Vec<&str> = remote_path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() <= 1 {
            return Ok(());
        }

        let mut current_path = String::new();
        for part in &parts[..parts.len() - 1] {
            current_path.push('/');
            current_path.push_str(part);

            let url = format!("{}{}", self.url.trim_end_matches('/'), current_path);
            let res = self.client.request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.password))
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() && status.as_u16() != 405 {
                return Err(StorageError::Provider(format!("Failed to create directory {}: {}", url, status)));
            }
        }
        Ok(())
    }
}

/// Percent-decodes a URL-encoded string.
///
/// # Arguments
/// * `s` - The URL-encoded string slice.
///
/// # Returns
/// The decoded String.
fn percent_decode(s: &str) -> String {
    let mut decoded = String::new();
    let mut bytes = s.as_bytes().iter();
    while let Some(&b) = bytes.next() {
        if b == b'%' {
            if let (Some(&h), Some(&l)) = (bytes.next(), bytes.next()) {
                if let Ok(hex) = String::from_utf8(vec![h, l]) {
                    if let Ok(num) = u8::from_str_radix(&hex, 16) {
                        decoded.push(num as char);
                        continue;
                    }
                }
            }
        }
        decoded.push(b as char);
    }
    decoded
}

fn parse_propfind_response(xml: &str) -> Result<Vec<(String, u64, bool)>, StorageError> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut items = Vec::new();

    let mut current_href = None;
    let mut current_size = 0;
    let mut current_is_dir = false;
    let mut active_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                active_tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if active_tag == "collection" {
                    current_is_dir = true;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "collection" {
                    current_is_dir = true;
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                active_tag.clear();
                if name == "response" {
                    if let Some(href) = current_href.take() {
                        items.push((href, current_size, current_is_dir));
                    }
                    current_size = 0;
                    current_is_dir = false;
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().into_owned();
                if active_tag == "href" {
                    current_href = Some(text);
                } else if active_tag == "getcontentlength" {
                    current_size = text.parse::<u64>().unwrap_or(0);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(StorageError::Provider(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(items)
}

#[async_trait]
impl StorageBackend for WebDAVProvider {
    fn name(&self) -> &str {
        "WebDAV"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);
        self.ensure_parent_dirs(&clean_path).await?;

        info!("[{}] Real upload starting for '{}'", self.name(), clean_path);
        let file_content = fs::read(local_path).await?;

        let upload_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
        let res = self.client.put(&upload_url)
            .basic_auth(&self.credentials.username, Some(&self.credentials.password))
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
        let clean_path = self.format_path(remote_path);

        let download_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
        let res = self.client.get(&download_url)
            .basic_auth(&self.credentials.username, Some(&self.credentials.password))
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
        let clean_path = self.format_path(remote_path);

        let delete_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
        let res = self.client.delete(&delete_url)
            .basic_auth(&self.credentials.username, Some(&self.credentials.password))
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "delete").await);
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let clean_path = self.format_path(remote_path);

        let mut list_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
        if !list_url.ends_with('/') {
            list_url.push('/');
        }
        let res = self.client.request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &list_url)
            .basic_auth(&self.credentials.username, Some(&self.credentials.password))
            .header("Depth", "1")
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(parse_response_error(res, self.name(), "list").await);
        }

        let body = res.text().await?;
        let items = parse_propfind_response(&body)?;

        let mut storage_items = Vec::new();
        let mut first = true;
        for (href, size, is_dir) in items {
            if first {
                first = false;
                continue;
            }

            let decoded = percent_decode(&href);
            let clean_href = decoded.trim_end_matches('/');
            let name = clean_href.split('/').last().unwrap_or("").to_string();

            if !name.is_empty() {
                storage_items.push(StorageItem {
                    path: PathBuf::from(name),
                    size,
                    modified: std::time::SystemTime::now(),
                    is_dir,
                });
            }
        }

        Ok(storage_items)
    }
}
