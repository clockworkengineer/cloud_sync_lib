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
    /// Creates a new `SFTPProvider` instance.
    pub fn new(creds: SFTPCredentials) -> Self {
        Self { creds }
    }

    /// Establishes a TCP connection, handshakes SSH, and authenticates.
    fn connect(&self) -> Result<Session, StorageError> {
        let port = self.creds.port.unwrap_or(22);
        let addr = format!("{}:{}", self.creds.host, port);
        let tcp = TcpStream::connect(&addr)?;
        
        let mut sess = Session::new().map_err(|e| StorageError::Provider(e.to_string()))?;
        sess.set_tcp_stream(tcp);
        sess.handshake().map_err(|e| StorageError::Provider(e.to_string()))?;

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
        let dest = self.creds.common.destination_folder.as_deref().unwrap_or("").trim_matches('/');
        let norm_path = remote_path.trim_matches('/');
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
            let sftp = sess.sftp().map_err(|e| StorageError::Provider(e.to_string()))?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real upload starting for '{}'", resolved);

            // Ensure parent directories exist
            if let Some(parent) = resolved_path.parent() {
                mkdir_p(&sftp, parent).map_err(|e| StorageError::Provider(e.to_string()))?;
            }

            let mut remote_file = sftp.create(resolved_path).map_err(|e| StorageError::Provider(e.to_string()))?;
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
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider(e.to_string()))?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);

            info!("[SFTP] Real download starting for '{}'", resolved);

            let mut remote_file = sftp.open(Path::new(&resolved)).map_err(|e| StorageError::Provider(e.to_string()))?;
            
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
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider(e.to_string()))?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real deletion starting for '{}'", resolved);

            let stat = match sftp.stat(resolved_path) {
                Ok(s) => s,
                Err(ref e) if e.code() == ssh2::ErrorCode::SFTP(2) => {
                    return Err(StorageError::NotFound(remote_path));
                }
                Err(e) => return Err(StorageError::Provider(e.to_string())),
            };

            if stat.is_dir() {
                sftp.rmdir(resolved_path).map_err(|e| StorageError::Provider(e.to_string()))?;
            } else {
                sftp.unlink(resolved_path).map_err(|e| StorageError::Provider(e.to_string()))?;
            }

            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let remote_path = remote_path.to_string();
        let provider = self.creds.clone();

        tokio::task::spawn_blocking(move || {
            let sftp_provider = SFTPProvider::new(provider);
            let sess = sftp_provider.connect()?;
            let sftp = sess.sftp().map_err(|e| StorageError::Provider(e.to_string()))?;
            let resolved = sftp_provider.resolve_remote_path(&remote_path);
            let resolved_path = Path::new(&resolved);

            info!("[SFTP] Real list directory starting for '{}'", resolved);

            let readdir_res = match sftp.readdir(resolved_path) {
                Ok(res) => res,
                Err(ref e) if e.code() == ssh2::ErrorCode::SFTP(2) => {
                    return Err(StorageError::NotFound(remote_path));
                }
                Err(e) => return Err(StorageError::Provider(e.to_string())),
            };

            let mut items = Vec::new();
            for (path, stat) in readdir_res {
                items.push(StorageItem {
                    path,
                    size: stat.size.unwrap_or(0),
                    modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(stat.mtime.unwrap_or(0)),
                    is_dir: stat.is_dir(),
                });
            }

            Ok(items)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    fn sync_mode(&self) -> super::SyncMode {
        use super::ProviderConfig;
        self.creds.sync_mode()
    }
}

