//! Amazon S3 & S3-Compatible storage backend provider implementation.
//!
//! Handles interaction with AWS S3, MinIO, Cloudflare R2, and other S3-compatible backends
//! using the lightweight `rust-s3` crate.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::S3Credentials;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Storage provider client for S3 and S3-Compatible storage servers.
pub struct S3Provider {
    /// The underlying S3 bucket client from the `s3` crate.
    bucket: s3::Bucket,
    /// Credentials configuration (bucket, access keys, endpoint).
    credentials: S3Credentials,
}

crate::impl_provider_builder!(S3Provider, S3ProviderBuilder, S3Credentials);

impl S3Provider {

    pub fn with_client_options(
        credentials: S3Credentials,
        _timeout: Option<std::time::Duration>,
        _custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        let region = if let Some(ref ep) = credentials.endpoint {
            s3::Region::Custom {
                region: credentials.region.clone(),
                endpoint: ep.clone(),
            }
        } else {
            credentials.region.parse::<s3::Region>().unwrap_or(s3::Region::UsEast1)
        };

        let creds = s3::creds::Credentials::new(
            Some(&credentials.access_key_id),
            Some(&credentials.secret_access_key),
            None,
            None,
            None,
        ).unwrap();

        let mut bucket = s3::Bucket::new(&credentials.bucket, region, creds).unwrap();
        // S3-compatible providers typically require path style queries
        if credentials.endpoint.is_some() {
            bucket.set_path_style();
        }

        Self {
            bucket,
            credentials,
        }
    }

    /// Configures custom endpoints, useful for mocking during tests.
    ///
    /// # Arguments
    /// * `url` - Custom S3 endpoint URL.
    ///
    /// # Returns
    /// The modified `S3Provider` instance.
    #[cfg(test)]
    pub fn with_endpoints(mut self, url: String) -> Self {
        // Construct region pointing to mock server URL
        let region = s3::Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: url,
        };
        let creds = s3::creds::Credentials::new(
            Some(&self.credentials.access_key_id),
            Some(&self.credentials.secret_access_key),
            None,
            None,
            None,
        ).unwrap();
        let mut bucket = s3::Bucket::new(&self.credentials.bucket, region, creds).unwrap();
        bucket.set_path_style();
        self.bucket = bucket;
        self
    }

    /// Formats the remote path, incorporating the optional destination folder prefix.
    ///
    /// # Arguments
    /// * `remote_path` - The relative destination path.
    ///
    /// # Returns
    /// The fully-resolved S3 object key string.
    fn format_path<'a>(&self, remote_path: &'a str) -> std::borrow::Cow<'a, str> {
        crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
    }
}

#[async_trait]
impl StorageBackend for S3Provider {
    fn name(&self) -> &str {
        "S3"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);
        info!("[{}] Real upload starting for '{}'", self.name(), clean_path);

        let file_content = fs::read(local_path).await?;
        let res = self.bucket.put_object(&clean_path, &file_content).await
            .map_err(|e| StorageError::Provider { message: format!("S3 upload error: {}", e), status: None })?;

        // HTTP status 200 or 201 indicates success
        let status_code = res.status_code();
        if status_code != 200 && status_code != 201 {
            return Err(StorageError::Provider { message: format!("S3 upload returned status code: {}", status_code), status: None });
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);

        let res = self.bucket.get_object(&clean_path).await
            .map_err(|e| StorageError::Provider { message: format!("S3 download error: {}", e), status: None })?;

        let status_code = res.status_code();
        if status_code != 200 {
            return Err(StorageError::Provider { message: format!("S3 download returned status code: {}", status_code), status: None });
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let bytes = res.bytes();
        fs::write(local_path, bytes).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);

        let res = self.bucket.delete_object(&clean_path).await
            .map_err(|e| StorageError::Provider { message: format!("S3 delete error: {}", e), status: None })?;

        let status_code = res.status_code();
        // 204 No Content or 200 OK are typical success status codes
        if status_code != 200 && status_code != 204 {
            return Err(StorageError::Provider { message: format!("S3 delete returned status code: {}", status_code), status: None });
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let mut prefix = self.format_path(remote_path).into_owned();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        // List files in the bucket matching the prefix
        let results = self.bucket.list(prefix.clone(), Some("/".to_string())).await
            .map_err(|e| StorageError::Provider { message: format!("S3 list error: {}", e), status: None })?;

        let mut items = Vec::new();
        for result in results {
            // Add files
            for object in result.contents {
                let key = object.key;
                // Exclude the folder prefix itself from the file list
                if key == prefix {
                    continue;
                }

                // Strip the queried prefix from the key to get the relative filename
                let relative_name = key.strip_prefix(&prefix).unwrap_or(&key).to_string();
                if relative_name.is_empty() {
                    continue;
                }

                let checksum = object.e_tag.as_ref().map(|tag| tag.trim_matches('"').to_string());

                items.push(StorageItem {
                    path: PathBuf::from(relative_name),
                    size: object.size,
                    modified: std::time::SystemTime::now(),
                    is_dir: false,
                    checksum,
                    permissions: None,
                });
            }

            // Add directories (common prefixes represent subfolders)
            for dir in result.common_prefixes.unwrap_or_default() {
                let key = dir.prefix;
                let relative_name = key.strip_prefix(&prefix).unwrap_or(&key)
                    .trim_end_matches('/')
                    .to_string();

                if relative_name.is_empty() {
                    continue;
                }

                items.push(StorageItem {
                    path: PathBuf::from(relative_name),
                    size: 0,
                    modified: std::time::SystemTime::now(),
                    is_dir: true,
                    checksum: None,
                    permissions: None,
                });
            }
        }

        Ok(items)
    }

    async fn compute_local_checksum(&self, local_path: &Path) -> Result<Option<String>, StorageError> {
        Ok(crate::checksum::compute_md5(local_path).await.ok())
    }
}

/// Builder for [`S3Provider`].
pub struct S3ProviderBuilder {
    pub credentials: S3Credentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl S3ProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: S3Credentials) -> Self {
        Self {
            credentials,
            timeout: None,
            custom_headers: None,
        }
    }

    /// Builds the provider.
    pub fn build(self) -> S3Provider {
        S3Provider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}
