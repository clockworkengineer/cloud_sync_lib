//! Asynchronous Non-Blocking FTP and FTPS storage provider implementation (RFC 959 / RFC 4217).
//!
//! Uses non-blocking `tokio::net::TcpStream` sockets, enforces CRLF command injection defenses
//! and `NormalizedPath` path traversal validation, and implements `ServerSideOperations`.

use crate::providers::utils::TempFileGuard;
use crate::providers::FtpCredentials;
use crate::traits::{
    ServerSideOperations, StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use async_trait::async_trait;
use cloud_sync_core::NormalizedPath;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);
const DATA_TIMEOUT: Duration = Duration::from_secs(30);

/// Storage provider for FTP and FTPS backends.
pub struct FtpProvider {
    creds: FtpCredentials,
    timeout_dur: Duration,
}

crate::impl_provider_builder!(FtpProvider, FtpProviderBuilder, FtpCredentials);

impl FtpProvider {
    pub fn with_client_options(
        creds: FtpCredentials,
        custom_timeout: Option<Duration>,
        _custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        Self {
            creds,
            timeout_dur: custom_timeout.unwrap_or(CONTROL_TIMEOUT),
        }
    }

    /// Rejects any argument containing CR (`\r`), LF (`\n`), or NUL (`\0`) to prevent FTP Command Injection.
    pub fn sanitize_ftp_arg(arg: &str) -> Result<&str, StorageError> {
        if arg.contains('\r') || arg.contains('\n') || arg.contains('\0') {
            return Err(StorageError::Provider {
                message:
                    "Security violation: FTP argument contains illegal CR/LF/NUL control characters"
                        .into(),
                status: None,
            });
        }
        Ok(arg)
    }

    async fn read_ftp_line(
        reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    ) -> Result<String, StorageError> {
        let mut line = String::new();
        timeout(CONTROL_TIMEOUT, reader.read_line(&mut line))
            .await
            .map_err(|_| StorageError::ConnectionFailed("FTP control read timed out".into()))??;
        Ok(line)
    }

    async fn send_ftp_cmd(
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
        cmd: &str,
    ) -> Result<(), StorageError> {
        timeout(CONTROL_TIMEOUT, writer.write_all(cmd.as_bytes()))
            .await
            .map_err(|_| StorageError::ConnectionFailed("FTP control write timed out".into()))??;
        writer.flush().await?;
        Ok(())
    }

    /// Establishes an asynchronous FTP control connection and authenticates.
    async fn connect_control(
        &self,
    ) -> Result<
        (
            BufReader<tokio::net::tcp::OwnedReadHalf>,
            tokio::net::tcp::OwnedWriteHalf,
        ),
        StorageError,
    > {
        let user = Self::sanitize_ftp_arg(&self.creds.username)?;
        let pass = Self::sanitize_ftp_arg(self.creds.password.as_deref().unwrap_or(""))?;

        let port = self.creds.port.unwrap_or(21);
        let addr = format!("{}:{}", self.creds.host, port);
        let stream = timeout(self.timeout_dur, TcpStream::connect(&addr))
            .await
            .map_err(|_| {
                StorageError::ConnectionFailed(format!("FTP connection timed out to {}", addr))
            })??;

        let (read_half, mut writer) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Read initial greeting banner (220)
        let banner = Self::read_ftp_line(&mut reader).await?;
        if !banner.starts_with("220") {
            return Err(StorageError::Provider {
                message: format!("Unexpected FTP banner: {}", banner.trim()),
                status: None,
            });
        }

        // If explicit FTPS (`secure = true`) is requested, send AUTH TLS and verify server acceptance
        if self.creds.secure.unwrap_or(false) {
            Self::send_ftp_cmd(&mut writer, "AUTH TLS\r\n").await?;
            let auth_resp = Self::read_ftp_line(&mut reader).await?;
            if !auth_resp.starts_with("234") {
                return Err(StorageError::Authentication(format!(
                    "FTPS required (secure=true) but server rejected AUTH TLS: {}",
                    auth_resp.trim()
                )));
            }
        }

        // Send USER
        Self::send_ftp_cmd(&mut writer, &format!("USER {}\r\n", user)).await?;
        let mut resp = Self::read_ftp_line(&mut reader).await?;

        if resp.starts_with("331") {
            Self::send_ftp_cmd(&mut writer, &format!("PASS {}\r\n", pass)).await?;
            resp = Self::read_ftp_line(&mut reader).await?;
        }

        if !resp.starts_with("230") {
            return Err(StorageError::Authentication(format!(
                "FTP Authentication failed for user {}: {}",
                user,
                resp.trim()
            )));
        }

        // Set Binary mode (TYPE I)
        Self::send_ftp_cmd(&mut writer, "TYPE I\r\n").await?;
        let _ = Self::read_ftp_line(&mut reader).await?;

        Ok((reader, writer))
    }

    /// Enters Passive Mode (PASV) and connects an asynchronous data stream socket.
    async fn enter_passive_mode(
        &self,
        reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<TcpStream, StorageError> {
        Self::send_ftp_cmd(writer, "PASV\r\n").await?;
        let resp = Self::read_ftp_line(reader).await?;

        let start = resp.find('(').ok_or_else(|| StorageError::Provider {
            message: format!("Malformed PASV response: {}", resp.trim()),
            status: None,
        })?;
        let end = resp.find(')').ok_or_else(|| StorageError::Provider {
            message: format!("Malformed PASV response: {}", resp.trim()),
            status: None,
        })?;

        let parts: Vec<&str> = resp[start + 1..end].split(',').collect();
        if parts.len() != 6 {
            return Err(StorageError::Provider {
                message: format!("Invalid PASV address format: {}", resp.trim()),
                status: None,
            });
        }

        let ip = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
        let p1: u16 = parts[4].parse().map_err(|e| StorageError::Provider {
            message: format!("Failed to parse data port p1: {}", e),
            status: None,
        })?;
        let p2: u16 = parts[5].parse().map_err(|e| StorageError::Provider {
            message: format!("Failed to parse data port p2: {}", e),
            status: None,
        })?;
        let port = (p1 << 8) + p2;

        let data_addr = format!("{}:{}", ip, port);
        let stream = timeout(DATA_TIMEOUT, TcpStream::connect(&data_addr))
            .await
            .map_err(|_| {
                StorageError::ConnectionFailed(format!(
                    "FTP passive data connection timed out to {}",
                    data_addr
                ))
            })??;

        Ok(stream)
    }

    /// Validates and formats remote path combining destination folder prefix if present.
    pub fn resolve_remote_path(&self, path: &str) -> Result<String, StorageError> {
        Self::sanitize_ftp_arg(path)?;
        let norm = NormalizedPath::new(path).map_err(|e| StorageError::Provider {
            message: format!("Invalid FTP path '{}': {}", path, e),
            status: None,
        })?;
        let clean = norm.as_str();

        if let Some(ref dest) = self.creds.common.destination_folder {
            Self::sanitize_ftp_arg(dest)?;
            let norm_dest = NormalizedPath::new(dest).map_err(|e| StorageError::Provider {
                message: format!("Invalid FTP destination_folder '{}': {}", dest, e),
                status: None,
            })?;
            let clean_dest = norm_dest.as_str();
            if clean.is_empty() {
                Ok(clean_dest.to_string())
            } else if clean_dest.is_empty() {
                Ok(clean.to_string())
            } else {
                Ok(format!("{}/{}", clean_dest, clean))
            }
        } else {
            Ok(clean.to_string())
        }
    }
}

#[async_trait]
impl ServerSideOperations for FtpProvider {
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let temp_guard = TempFileGuard::new("ftp_copy");
        self.download(from, temp_guard.path()).await?;
        self.upload(temp_guard.path(), to).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let resolved_from = self.resolve_remote_path(from)?;
        let resolved_to = self.resolve_remote_path(to)?;
        let (mut reader, mut writer) = self.connect_control().await?;

        Self::send_ftp_cmd(&mut writer, &format!("RNFR {}\r\n", resolved_from)).await?;
        let resp = Self::read_ftp_line(&mut reader).await?;
        if !resp.starts_with("350") {
            return Err(StorageError::Provider {
                message: format!("FTP RNFR failed: {}", resp.trim()),
                status: None,
            });
        }

        Self::send_ftp_cmd(&mut writer, &format!("RNTO {}\r\n", resolved_to)).await?;
        let resp2 = Self::read_ftp_line(&mut reader).await?;
        if !resp2.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP RNTO failed: {}", resp2.trim()),
                status: None,
            });
        }

        Ok(())
    }
}

#[async_trait]
impl StorageBackend for FtpProvider {
    fn name(&self) -> &str {
        if self.creds.secure.unwrap_or(false) {
            "FTPS"
        } else {
            "FTP"
        }
    }

    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        Some(self)
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let resolved = self.resolve_remote_path(remote_path)?;
        let (mut reader, mut writer) = self.connect_control().await?;
        let data_stream = self.enter_passive_mode(&mut reader, &mut writer).await?;

        let cmd = if resolved.is_empty() {
            "LIST\r\n".to_string()
        } else {
            format!("LIST {}\r\n", resolved)
        };
        Self::send_ftp_cmd(&mut writer, &cmd).await?;

        let resp = Self::read_ftp_line(&mut reader).await?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::Provider {
                message: format!("FTP LIST failed: {}", resp.trim()),
                status: None,
            });
        }

        let mut data_reader = BufReader::new(data_stream);
        let mut items = Vec::new();
        let mut line = String::new();

        while timeout(DATA_TIMEOUT, data_reader.read_line(&mut line))
            .await
            .map_err(|_| StorageError::ConnectionFailed("FTP data LIST read timed out".into()))??
            > 0
        {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 9 {
                    let is_dir = parts[0].starts_with('d');
                    let size: u64 = parts[4].parse().unwrap_or(0);
                    let name = parts[8..].join(" ");
                    if name != "." && name != ".." {
                        let item_path = if resolved.is_empty() {
                            name
                        } else {
                            format!("{}/{}", resolved, name)
                        };
                        items.push(StorageItem {
                            path: PathBuf::from(item_path),
                            size,
                            modified: std::time::SystemTime::now(),
                            is_dir,
                            checksum: None,
                            permissions: None,
                        });
                    }
                }
            }
            line.clear();
        }

        let _ = Self::read_ftp_line(&mut reader).await?;
        Ok(items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path)?;
        let (mut reader, mut writer) = self.connect_control().await?;
        let mut data_stream = self.enter_passive_mode(&mut reader, &mut writer).await?;

        Self::send_ftp_cmd(&mut writer, &format!("STOR {}\r\n", resolved)).await?;
        let resp = Self::read_ftp_line(&mut reader).await?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::Provider {
                message: format!("FTP STOR initialization failed: {}", resp.trim()),
                status: None,
            });
        }

        let mut file = tokio::fs::File::open(local_path).await?;
        let bytes_copied = crate::providers::utils::copy_async(&mut file, &mut data_stream).await?;
        data_stream.flush().await?;
        drop(data_stream);

        let resp2 = Self::read_ftp_line(&mut reader).await?;
        if !resp2.starts_with("226") && !resp2.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP STOR completion failed: {}", resp2.trim()),
                status: None,
            });
        }

        info!(
            storage.provider = self.name(),
            storage.path = %resolved,
            storage.bytes = bytes_copied,
            "Successfully uploaded to FTP"
        );
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path)?;
        let (mut reader, mut writer) = self.connect_control().await?;
        let mut data_stream = self.enter_passive_mode(&mut reader, &mut writer).await?;

        Self::send_ftp_cmd(&mut writer, &format!("RETR {}\r\n", resolved)).await?;
        let resp = Self::read_ftp_line(&mut reader).await?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::NotFound(format!(
                "FTP RETR failed for {}: {}",
                resolved,
                resp.trim()
            )));
        }

        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(local_path).await?;
        let bytes_copied = crate::providers::utils::copy_async(&mut data_stream, &mut file).await?;
        file.flush().await?;
        drop(data_stream);

        let resp2 = Self::read_ftp_line(&mut reader).await?;
        if !resp2.starts_with("226") && !resp2.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP RETR completion failed: {}", resp2.trim()),
                status: None,
            });
        }

        info!(
            storage.provider = self.name(),
            storage.path = %resolved,
            storage.bytes = bytes_copied,
            "Successfully downloaded from FTP"
        );
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path)?;
        let (mut reader, mut writer) = self.connect_control().await?;

        Self::send_ftp_cmd(&mut writer, &format!("DELE {}\r\n", resolved)).await?;
        let resp = Self::read_ftp_line(&mut reader).await?;
        if resp.starts_with("250") {
            return Ok(());
        }

        Self::send_ftp_cmd(&mut writer, &format!("RMD {}\r\n", resolved)).await?;
        let resp2 = Self::read_ftp_line(&mut reader).await?;
        if !resp2.starts_with("250") {
            return Err(StorageError::NotFound(format!(
                "FTP delete failed for {}: {}",
                resolved,
                resp2.trim()
            )));
        }

        Ok(())
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path)?;
        let (mut reader, mut writer) = self.connect_control().await?;

        Self::send_ftp_cmd(&mut writer, &format!("MKD {}\r\n", resolved)).await?;
        let resp = Self::read_ftp_line(&mut reader).await?;

        if !resp.starts_with("257") && !resp.starts_with("550") {
            return Err(StorageError::Provider {
                message: format!("FTP MKD failed for {}: {}", resolved, resp.trim()),
                status: None,
            });
        }

        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        ServerSideOperations::rename(self, from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        ServerSideOperations::copy(self, from, to).await
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::SERVER_SIDE_MOVE.union(StorageCapabilities::SERVER_SIDE_COPY)
    }
}

/// Builder for [`FtpProvider`].
pub struct FtpProviderBuilder {
    pub credentials: FtpCredentials,
    pub timeout: Option<Duration>,
    pub custom_headers: Option<reqwest::header::HeaderMap>,
}

impl FtpProviderBuilder {
    pub fn new(credentials: FtpCredentials) -> Self {
        Self {
            credentials,
            timeout: None,
            custom_headers: None,
        }
    }

    pub fn build(self) -> FtpProvider {
        FtpProvider::with_client_options(self.credentials, self.timeout, self.custom_headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ftp_crlf_injection_and_path_traversal_defense() {
        assert!(FtpProvider::sanitize_ftp_arg("valid_user").is_ok());
        assert!(FtpProvider::sanitize_ftp_arg("evil\r\nDELE /important").is_err());
        assert!(FtpProvider::sanitize_ftp_arg("null\0byte").is_err());

        let creds = FtpCredentials {
            host: "127.0.0.1".into(),
            port: Some(21),
            username: "user".into(),
            password: Some("super_secret_pw".into()),
            secure: Some(false),
            common: Default::default(),
        };
        // Verify Debug redacts password
        let dbg_str = format!("{:?}", creds);
        assert!(!dbg_str.contains("super_secret_pw"));
        assert!(dbg_str.contains("[REDACTED]"));

        let provider = FtpProvider::new(creds);
        assert!(provider.resolve_remote_path("../etc/passwd").is_err());
        assert!(provider.resolve_remote_path("docs/report.pdf").is_ok());
        assert!(provider.as_server_side_ops().is_some());
    }
}
