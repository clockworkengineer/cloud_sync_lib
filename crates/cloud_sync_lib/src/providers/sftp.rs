//! SFTP cloud storage provider implementation.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::SFTPCredentials;
use std::path::Path;
use std::net::TcpStream;
use std::io::{Read, Write};
use ssh2::{Session, Sftp};
use async_trait::async_trait;
use tracing::info;

/// Storage provider for SFTP storage backends.
pub struct SFTPProvider {
    creds: SFTPCredentials,
}

impl SFTPProvider {
    /// Returns a new builder to configure the provider.
    pub fn builder(credentials: SFTPCredentials) -> SFTPProviderBuilder {
        SFTPProviderBuilder::new(credentials)
    }

    /// Creates a new `SFTPProvider` instance.
    pub fn new(creds: SFTPCredentials) -> Self {
        Self { creds }
    }

    /// Establishes a TCP connection, handshakes SSH, and authenticates.
    fn connect(&self) -> Result<Session, StorageError> {
        let port = self.creds.port.unwrap_or(22);
        let addr = format!("{}:{}", self.creds.host, port);
        let tcp = TcpStream::connect(&addr)?;
        
        let mut sess = Session::new().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
        sess.set_tcp_stream(tcp);
        sess.handshake().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;

        let has_key = self.creds.private_key_path.as_ref().is_some_and(|p| !p.is_empty());
        if has_key {
            let path = Path::new(self.creds.private_key_path.as_ref().unwrap());
            sess.userauth_pubkey_file(&self.creds.username, None, path, None)
                .map_err(|e| StorageError::Authentication(e.to_string()))?;
        } else if let Some(ref password) = self.creds.password {
            sess.userauth_password(&self.creds.username, password)
                .map_err(|e| StorageError::Authentication(e.to_string()))?;
        } else {
            return Err(StorageError::Authentication("No password or private key provided for SFTP".to_string()));
        }

        Ok(sess)
    }

    /// Helper to resolve the complete remote path using the destination folder prefix.
    fn resolve_remote_path(&self, remote_path: &str) -> String {
        let dest_norm = super::utils::normalize_remote_path(self.creds.common.destination_folder.as_deref().unwrap_or(""));
        let dest = dest_norm.trim_matches('/');
        let norm_path_str = super::utils::normalize_remote_path(remote_path);
        let norm_path = norm_path_str.trim_matches('/');
        if dest.is_empty() {
            norm_path.to_string()
        } else if norm_path.is_empty() {
            dest.to_string()
        } else {
            format!("{}/{}", dest, norm_path)
        }
    }
}

/// Recursively creates remote directories.
fn mkdir_p(sftp: &Sftp, path: &Path) -> Result<(), ssh2::Error> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        mkdir_p(sftp, parent)?;
    }
    let _ = sftp.mkdir(path, 0o755);
    Ok(())
}

#[async_trait]
impl StorageBackend for SFTPProvider {
    fn name(&self) -> &str {
        "SFTP"
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real upload starting for '{}'", resolved);

            // Ensure parent directories exist
            if let Some(parent) = resolved_path.parent() {
                mkdir_p(&sftp, parent).map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            }

            let mut remote_file = sftp.create(resolved_path).map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            let mut local_file = std::fs::File::open(&local_path)?;
            
            let mut buffer = vec![0; 16384];
            loop {
                let bytes_read = local_file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                remote_file.write_all(&buffer[..bytes_read])?;
            }

            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);

            info!("[SFTP] Real download starting for '{}'", resolved);

            let mut remote_file = sftp.open(Path::new(&resolved)).map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut local_file = std::fs::File::create(&local_path)?;

            let mut buffer = vec![0; 16384];
            loop {
                let bytes_read = remote_file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                local_file.write_all(&buffer[..bytes_read])?;
            }

            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real deletion starting for '{}'", resolved);

            let stat = match sftp.stat(resolved_path) {
                Ok(s) => s,
                Err(ref e) if e.code() == ssh2::ErrorCode::SFTP(2) => {
                    return Err(StorageError::NotFound(remote_path));
                }
                Err(e) => return Err(StorageError::Provider { message: e.to_string(), status: None }),
            };

            if stat.is_dir() {
                sftp.rmdir(resolved_path).map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            } else {
                sftp.unlink(resolved_path).map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            }

            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real list directory starting for '{}'", resolved);

            let readdir_res = match sftp.readdir(resolved_path) {
                Ok(res) => res,
                Err(ref e) if e.code() == ssh2::ErrorCode::SFTP(2) => {
                    return Err(StorageError::NotFound(remote_path));
                }
                Err(e) => return Err(StorageError::Provider { message: e.to_string(), status: None }),
            };

            let mut items = Vec::new();
            for (path, stat) in readdir_res {
                if let Some(filename) = path.file_name() {
                    let filename_str = filename.to_string_lossy();
                    if filename_str == "." || filename_str == ".." {
                        continue;
                    }
                    let relative_path = Path::new(&remote_path).join(filename);
                    items.push(StorageItem {
                        path: relative_path,
                        size: stat.size.unwrap_or(0),
                        modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(stat.mtime.unwrap_or(0)),
                        is_dir: stat.is_dir(),
                        checksum: None,
                    });
                }
            }

            Ok(items)
        })
        .await
        .map_err(|e| StorageError::Provider { message: e.to_string(), status: None })?
    }

}



/// Builder for [`SFTPProvider`].
pub struct SFTPProviderBuilder {
    pub credentials: SFTPCredentials,
    pub timeout: Option<std::time::Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl SFTPProviderBuilder {
    /// Creates a new builder with the required credentials.
    pub fn new(credentials: SFTPCredentials) -> Self {
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
    pub fn build(self) -> SFTPProvider {
        SFTPProvider::new(self.credentials)
    }
}
