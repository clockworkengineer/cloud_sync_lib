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
    bucket: s3::Bucket,
    credentials: S3Credentials,
}

impl S3Provider {
    pub fn new(credentials: S3Credentials) -> Self {
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

    fn format_path(&self, remote_path: &str) -> String {
        let clean_path = remote_path.trim_start_matches('/');
        if let Some(ref dest_folder) = self.credentials.destination_folder {
            let clean_dest = dest_folder.trim_matches('/');
            if !clean_dest.is_empty() {
                if clean_path.is_empty() {
                    return clean_dest.to_string();
                } else {
                    return format!("{}/{}", clean_dest, clean_path);
                }
            }
        }
        clean_path.to_string()
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
            .map_err(|e| StorageError::Provider(format!("S3 upload error: {}", e)))?;

        // HTTP status 200 or 201 indicates success
        let status_code = res.status_code();
        if status_code != 200 && status_code != 201 {
            return Err(StorageError::Provider(format!("S3 upload returned status code: {}", status_code)));
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let clean_path = self.format_path(remote_path);

        let res = self.bucket.get_object(&clean_path).await
            .map_err(|e| StorageError::Provider(format!("S3 download error: {}", e)))?;

        let status_code = res.status_code();
        if status_code != 200 {
            return Err(StorageError::Provider(format!("S3 download returned status code: {}", status_code)));
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
            .map_err(|e| StorageError::Provider(format!("S3 delete error: {}", e)))?;

        let status_code = res.status_code();
        // 204 No Content or 200 OK are typical success status codes
        if status_code != 200 && status_code != 204 {
            return Err(StorageError::Provider(format!("S3 delete returned status code: {}", status_code)));
        }

        Ok(())
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let mut prefix = self.format_path(remote_path);
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        // List files in the bucket matching the prefix
        let results = self.bucket.list(prefix.clone(), Some("/".to_string())).await
            .map_err(|e| StorageError::Provider(format!("S3 list error: {}", e)))?;

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

                items.push(StorageItem {
                    path: PathBuf::from(relative_name),
                    size: object.size,
                    modified: std::time::SystemTime::now(),
                    is_dir: false,
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
                });
            }
        }

        Ok(items)
    }
}
