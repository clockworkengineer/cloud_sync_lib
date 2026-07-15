//! Nextcloud storage backend provider implementation.
//!
//! Handles interaction with Nextcloud servers using the WebDAV endpoint:
//! `/remote.php/dav/files/{username}/`

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::NextcloudCredentials;
use crate::providers::utils::parse_response_error;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;
use quick_xml::events::Event;
use quick_xml::Reader;

/// Storage provider client for Nextcloud storage servers.
pub struct NextcloudProvider {
    /// The HTTP client for making API requests.
    client: reqwest::Client,
    /// Credentials configuration.
    credentials: NextcloudCredentials,
    /// Base WebDAV URL of the Nextcloud server (includes username suffix).
    url: String,
}

impl NextcloudProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: NextcloudCredentials) -> NextcloudProviderBuilder {
        NextcloudProviderBuilder::new(credentials)
    }

    /// Creates a new `NextcloudProvider` using the provided credentials.
    pub fn new(credentials: NextcloudCredentials) -> Self {
        let mut base_url = credentials.url.trim_end_matches('/').to_string();
        base_url = format!("{}/remote.php/dav/files/{}", base_url, credentials.username);
        Self {
            client: super::utils::build_http_client(),
            url: base_url,
            credentials,
        }
    }

    /// Configures custom endpoints, useful for mocking during tests.
    #[cfg(test)]
    pub fn with_endpoints(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    fn format_path(&self, remote_path: &str) -> String {
        crate::providers::utils::format_absolute_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }

    /// Ensures that parent directories exist on the Nextcloud server.
    async fn ensure_parent_dirs(&self, remote_path: &str) -> Result<(), StorageError> {
        let parts: Vec<&str> = remote_path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() <= 1 {
            return Ok(());
        }

        let mut current_path = String::new();
        for part in &parts[..parts.len() - 1] {
            current_path.push('/');
            current_path.push_str(part);

            let url = format!("{}{}", self.url, current_path);
            let resp = self.client.request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() && status.as_u16() != 405 {
                return Err(StorageError::Provider { message: format!("Failed to create directory {}: {}", url, status), status: None });
            }
        }

        Ok(())
    }
}

/// Percent-decodes a URL-encoded string.
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
            Err(e) => return Err(StorageError::Provider { message: format!("XML parse error: {}", e), status: None }),
            _ => {}
        }
        buf.clear();
    }

    Ok(items)
}

#[async_trait]
impl StorageBackend for NextcloudProvider {
    fn name(&self) -> &str {
        "Nextcloud"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "upload", || async {
            let clean_path = self.format_path(remote_path);
            self.ensure_parent_dirs(&clean_path).await?;

            info!("[Nextcloud] Real upload starting for '{}'", clean_path);
            let file_content = fs::read(local_path).await?;

            let upload_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
            let res = self.client.put(&upload_url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
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

            let download_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
            let res = self.client.get(&download_url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
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

            let delete_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
            let res = self.client.delete(&delete_url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(parse_response_error(res, self.name(), "delete").await);
            }

            Ok(())
        }).await
    }

    /// Creates a directory folder on Nextcloud using WebDAV MKCOL method.
    ///
    /// # Arguments
    /// * `remote_path` - The folder path relative to the sync root.
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        super::utils::execute_with_retry(self.name(), "create_folder", || async {
            let clean_path = self.format_path(remote_path);
            if clean_path.is_empty() {
                return Ok(());
            }

            let create_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
            let res = self.client.request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &create_url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() {
                let err_text = res.text().await.unwrap_or_default();
                if status.as_u16() == 405 || status.as_u16() == 409 || err_text.contains("conflict") || err_text.contains("exists") {
                    return Ok(());
                }
                return Err(StorageError::Provider {
                    message: format!("Failed to create folder: {}", err_text),
                    status: Some(status.as_u16()),
                });
            }

            Ok(())
        }).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        super::utils::execute_with_retry(self.name(), "list", || async {
            let clean_path = self.format_path(remote_path);

            let mut list_url = format!("{}{}", self.url.trim_end_matches('/'), clean_path);
            if !list_url.ends_with('/') {
                list_url.push('/');
            }
            let res = self.client.request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &list_url)
                .basic_auth(&self.credentials.username, Some(&self.credentials.app_password))
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
                let name = clean_href.split('/').next_back().unwrap_or("").to_string();

                if !name.is_empty() {
                    let rel_path = if remote_path.is_empty() {
                        name
                    } else {
                        format!("{}/{}", remote_path, name)
                    };

                    storage_items.push(StorageItem {
                        path: PathBuf::from(rel_path),
                        size,
                        modified: std::time::SystemTime::now(),
                        is_dir,
                        checksum: None,
                    });
                }
            }

            Ok(storage_items)
        }).await
    }

}



/// Builder for [`NextcloudProvider`].
pub struct NextcloudProviderBuilder {
    pub credentials: NextcloudCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl NextcloudProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: NextcloudCredentials) -> Self {
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
    pub fn build(self) -> NextcloudProvider {
        NextcloudProvider::new(self.credentials)
    }
}
