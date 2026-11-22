//! HTTP / HTTPS Remote Directory & REST File Server Provider (Rclone `http` equivalent).
//!
//! Supports listing and synchronizing files with HTTP/HTTPS web servers (Apache, Nginx,
//! Caddy, Python `http.server`, or REST file endpoints), parsing both HTML `<a href="...">`
//! autoindex pages and JSON directory indices, and supporting `GET`, `HEAD`, `PUT`, and `DELETE`.

use crate::providers::HttpCredentials;
use crate::traits::{StorageBackend, StorageCapabilities, StorageError, StorageItem};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::info;

/// Storage provider for HTTP / HTTPS file servers and autoindex directories.
pub struct HttpProvider {
    client: reqwest::Client,
    credentials: HttpCredentials,
}

crate::impl_provider_builder!(HttpProvider, HttpProviderBuilder, HttpCredentials);

impl HttpProvider {
    pub fn with_client_options(
        credentials: HttpCredentials,
        timeout: Option<std::time::Duration>,
        custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        let client = crate::providers::utils::build_http_client(timeout, custom_headers);
        Self {
            client,
            credentials,
        }
    }

    fn resolve_url(&self, remote_path: &str) -> String {
        let base = self.credentials.url.trim_end_matches('/');
        let rel = crate::providers::utils::format_relative_path(
            remote_path,
            self.credentials.common.destination_folder.as_deref(),
        );
        let clean = rel.trim_matches('/');
        if clean.is_empty() {
            format!("{}/", base)
        } else {
            format!("{}/{}", base, clean)
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref token) = self.credentials.bearer_token {
            if !token.is_empty() {
                return req.bearer_auth(token);
            }
        }
        if let Some(ref user) = self.credentials.username {
            if !user.is_empty() {
                return req.basic_auth(user, self.credentials.password.as_deref());
            }
        }
        req
    }

    /// Parses links from an HTML autoindex directory listing (`<a href="...">`).
    pub fn parse_html_index(html: &str, parent_rel: &str) -> Vec<StorageItem> {
        let mut items = Vec::new();
        let mut search_slice = html;
        let clean_parent = parent_rel.trim_matches('/');

        while let Some(href_idx) = search_slice
            .find("href=\"")
            .or_else(|| search_slice.find("HREF=\""))
        {
            let after_href = &search_slice[href_idx + 6..];
            if let Some(end_quote) = after_href.find('"') {
                let raw_link = &after_href[..end_quote];
                search_slice = &after_href[end_quote + 1..];

                // Skip query links, parent links, absolute external URLs, and anchors
                if raw_link.is_empty()
                    || raw_link.starts_with('?')
                    || raw_link.starts_with('#')
                    || raw_link.starts_with('/')
                    || raw_link == "../"
                    || raw_link == "./"
                    || raw_link == ".."
                    || raw_link == "."
                    || raw_link.contains("://")
                {
                    continue;
                }

                let is_dir = raw_link.ends_with('/');
                let clean_name = raw_link.trim_end_matches('/');
                if clean_name.is_empty() || clean_name.contains('/') {
                    continue;
                }

                let decoded_name = clean_name.replace("%20", " ");
                let full_rel = if clean_parent.is_empty() {
                    decoded_name
                } else {
                    format!("{}/{}", clean_parent, decoded_name)
                };

                items.push(StorageItem {
                    path: PathBuf::from(full_rel),
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                    is_dir,
                    checksum: None,
                    permissions: None,
                });
            } else {
                break;
            }
        }

        items
    }

    /// Parses a JSON directory listing (e.g. Caddy / Nginx `autoindex_format json`).
    pub fn parse_json_index(json_text: &str, parent_rel: &str) -> Option<Vec<StorageItem>> {
        let val: serde_json::Value = serde_json::from_str(json_text).ok()?;
        let arr = val.as_array()?;
        let clean_parent = parent_rel.trim_matches('/');
        let mut items = Vec::new();

        for entry in arr {
            let name = entry.get("name").and_then(|v| v.as_str())?;
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = entry
                .get("is_dir")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    entry
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(|t| t == "directory" || t == "dir")
                })
                .unwrap_or_else(|| name.ends_with('/'));
            let size = entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let clean_name = name.trim_end_matches('/');
            let full_rel = if clean_parent.is_empty() {
                clean_name.to_string()
            } else {
                format!("{}/{}", clean_parent, clean_name)
            };

            items.push(StorageItem {
                path: PathBuf::from(full_rel),
                size,
                modified: SystemTime::UNIX_EPOCH,
                is_dir,
                checksum: entry
                    .get("checksum")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                permissions: None,
            });
        }

        Some(items)
    }
}

#[async_trait]
impl StorageBackend for HttpProvider {
    fn name(&self) -> &str {
        "HTTP"
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let mut url = self.resolve_url(remote_path);
        if !url.ends_with('/') {
            url.push('/');
        }

        let req = self.apply_auth(self.client.get(&url));
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(crate::providers::utils::translate_status_code_error(
                resp.status().as_u16(),
                "HTTP",
                "list",
                Some(&url),
            ));
        }

        let body = resp.text().await?;
        let trimmed = body.trim();
        if trimmed.starts_with('[') {
            if let Some(items) = Self::parse_json_index(trimmed, remote_path) {
                return Ok(items);
            }
        }

        let mut items = Self::parse_html_index(&body, remote_path);

        // Probe file sizes via HEAD requests for discovered files
        for item in &mut items {
            if !item.is_dir {
                let item_path_str = item.path.to_string_lossy().replace('\\', "/");
                let file_url = self.resolve_url(&item_path_str);
                if let Ok(head_resp) = self.apply_auth(self.client.head(&file_url)).send().await {
                    if head_resp.status().is_success() {
                        if let Some(len) = head_resp
                            .headers()
                            .get(reqwest::header::CONTENT_LENGTH)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                        {
                            item.size = len;
                        }
                        if let Some(etag) = head_resp
                            .headers()
                            .get(reqwest::header::ETAG)
                            .and_then(|v| v.to_str().ok())
                        {
                            item.checksum = Some(etag.trim_matches('"').to_string());
                        }
                    }
                }
            }
        }

        Ok(items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let url = self.resolve_url(remote_path);
        let bytes = tokio::fs::read(local_path).await?;
        let req = self.apply_auth(self.client.put(&url)).body(bytes);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            return Err(crate::providers::utils::translate_status_code_error(
                resp.status().as_u16(),
                "HTTP",
                "upload",
                Some(&url),
            ));
        }

        info!("Successfully uploaded to HTTP server: {}", remote_path);
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let url = self.resolve_url(remote_path);
        let req = self.apply_auth(self.client.get(&url));
        let resp = req.send().await?;

        if !resp.status().is_success() {
            return Err(crate::providers::utils::translate_status_code_error(
                resp.status().as_u16(),
                "HTTP",
                "download",
                Some(&url),
            ));
        }

        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = resp.bytes().await?;
        tokio::fs::write(local_path, &bytes).await?;

        info!("Successfully downloaded from HTTP server: {}", remote_path);
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let url = self.resolve_url(remote_path);
        let req = self.apply_auth(self.client.delete(&url));
        let resp = req.send().await?;

        if !resp.status().is_success() {
            return Err(crate::providers::utils::translate_status_code_error(
                resp.status().as_u16(),
                "HTTP",
                "delete",
                Some(&url),
            ));
        }
        Ok(())
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let mut url = self.resolve_url(remote_path);
        if !url.ends_with('/') {
            url.push('/');
        }
        let req = self
            .apply_auth(self.client.put(&url))
            .body(Vec::<u8>::new());
        let _ = req.send().await;
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let temp_guard = crate::providers::utils::TempFileGuard::new("http_move");
        self.download(from, temp_guard.path()).await?;
        self.upload(temp_guard.path(), to).await?;
        self.delete(from).await
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::STREAMING_READ
    }
}

/// Builder for [`HttpProvider`].
pub struct HttpProviderBuilder {
    pub credentials: HttpCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl HttpProviderBuilder {
    pub fn new(credentials: HttpCredentials) -> Self {
        Self {
            credentials,
            timeout: None,
            custom_headers: None,
        }
    }

    pub fn build(self) -> HttpProvider {
        HttpProvider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_parse_html_autoindex() {
        let html = r#"
        <html><body>
        <h1>Index of /releases/</h1>
        <a href="../">../</a>
        <a href="docs/">docs/</a>
        <a href="archive.tar.gz">archive.tar.gz</a>
        <a href="readme%20file.txt">readme file.txt</a>
        </body></html>
        "#;

        let items = HttpProvider::parse_html_index(html, "releases");
        assert_eq!(items.len(), 3);
        assert!(items[0].is_dir);
        assert_eq!(items[0].path, PathBuf::from("releases/docs"));
        assert!(!items[1].is_dir);
        assert_eq!(items[1].path, PathBuf::from("releases/archive.tar.gz"));
        assert_eq!(items[2].path, PathBuf::from("releases/readme file.txt"));
    }

    #[tokio::test]
    async fn test_http_provider_mock_lifecycle() {
        let server = MockServer::start().await;

        let index_json = serde_json::json!([
            { "name": "notes.txt", "size": 12, "is_dir": false },
            { "name": "subdir", "size": 0, "is_dir": true }
        ]);

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&index_json))
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/notes.txt"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/notes.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello http!"))
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/notes.txt"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let creds = HttpCredentials {
            url: server.uri(),
            username: Some("admin".into()),
            password: Some("secret".into()),
            bearer_token: None,
            common: Default::default(),
        };

        let provider = HttpProvider::new(creds);
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].size, 12);

        let dir = tempfile::tempdir().unwrap();
        let up_file = dir.path().join("up.txt");
        tokio::fs::write(&up_file, b"hello http!").await.unwrap();

        provider.upload(&up_file, "notes.txt").await.unwrap();

        let down_file = dir.path().join("down.txt");
        provider.download("notes.txt", &down_file).await.unwrap();
        let downloaded = tokio::fs::read_to_string(&down_file).await.unwrap();
        assert_eq!(downloaded, "hello http!");

        provider.delete("notes.txt").await.unwrap();
    }
}
