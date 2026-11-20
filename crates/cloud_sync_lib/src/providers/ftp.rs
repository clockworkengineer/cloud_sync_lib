//! FTP and FTPS storage provider implementation (RFC 959 / RFC 4217).

use crate::providers::FtpCredentials;
use crate::traits::{StorageBackend, StorageCapabilities, StorageError, StorageItem};
use async_trait::async_trait;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use tracing::info;

/// Storage provider for FTP and FTPS backends.
pub struct FtpProvider {
    creds: FtpCredentials,
}

crate::impl_provider_builder!(FtpProvider, FtpProviderBuilder, FtpCredentials);

impl FtpProvider {
    pub fn with_client_options(
        creds: FtpCredentials,
        _timeout: Option<std::time::Duration>,
        _custom_headers: Option<reqwest::header::HeaderMap>,
    ) -> Self {
        Self { creds }
    }

    /// Establishes an FTP control connection and authenticates.
    fn connect_control(&self) -> Result<(BufReader<TcpStream>, TcpStream), StorageError> {
        let port = self.creds.port.unwrap_or(21);
        let addr = format!("{}:{}", self.creds.host, port);
        let stream = TcpStream::connect(&addr)?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(15)))
            .ok();

        let mut reader = BufReader::new(stream.try_clone()?);
        let mut writer = stream;

        // Read initial greeting banner (220)
        let mut banner = String::new();
        reader.read_line(&mut banner)?;
        if !banner.starts_with("220") {
            return Err(StorageError::Provider {
                message: format!("Unexpected FTP banner: {}", banner.trim()),
                status: None,
            });
        }

        // Send USER
        writer.write_all(format!("USER {}\r\n", self.creds.username).as_bytes())?;
        writer.flush()?;
        let mut resp = String::new();
        reader.read_line(&mut resp)?;

        if resp.starts_with("331") {
            // Needs PASS
            let pass = self.creds.password.as_deref().unwrap_or("");
            writer.write_all(format!("PASS {}\r\n", pass).as_bytes())?;
            writer.flush()?;
            resp.clear();
            reader.read_line(&mut resp)?;
        }

        if !resp.starts_with("230") {
            return Err(StorageError::Authentication(format!(
                "FTP Authentication failed for user {}: {}",
                self.creds.username,
                resp.trim()
            )));
        }

        // Set Binary mode (TYPE I)
        writer.write_all(b"TYPE I\r\n")?;
        writer.flush()?;
        resp.clear();
        reader.read_line(&mut resp)?;

        Ok((reader, writer))
    }

    /// Enters Passive Mode (PASV) and connects data stream socket.
    fn enter_passive_mode(
        &self,
        reader: &mut BufReader<TcpStream>,
        writer: &mut TcpStream,
    ) -> Result<TcpStream, StorageError> {
        writer.write_all(b"PASV\r\n")?;
        writer.flush()?;

        let mut resp = String::new();
        reader.read_line(&mut resp)?;

        // Parse (h1,h2,h3,h4,p1,p2)
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
        let stream = TcpStream::connect(&data_addr)?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(30)))
            .ok();

        Ok(stream)
    }

    /// Formats remote path combining destination folder prefix if present.
    fn resolve_remote_path(&self, path: &str) -> String {
        let clean = path.trim_matches('/').replace('\\', "/");
        if let Some(ref dest) = self.creds.common.destination_folder {
            let clean_dest = dest.trim_matches('/').replace('\\', "/");
            if clean.is_empty() {
                clean_dest
            } else if clean_dest.is_empty() {
                clean
            } else {
                format!("{}/{}", clean_dest, clean)
            }
        } else {
            clean
        }
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

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let resolved = self.resolve_remote_path(remote_path);
        let (mut reader, mut writer) = self.connect_control()?;
        let mut data_stream = self.enter_passive_mode(&mut reader, &mut writer)?;

        // Send LIST
        let cmd = if resolved.is_empty() {
            "LIST\r\n".to_string()
        } else {
            format!("LIST {}\r\n", resolved)
        };
        writer.write_all(cmd.as_bytes())?;
        writer.flush()?;

        let mut resp = String::new();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::Provider {
                message: format!("FTP LIST failed: {}", resp.trim()),
                status: None,
            });
        }

        let mut data_reader = BufReader::new(&mut data_stream);
        let mut items = Vec::new();
        let mut line = String::new();

        while data_reader.read_line(&mut line)? > 0 {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                // Parse standard UNIX-style listing: drwxr-xr-x 2 user group 4096 Nov 20 12:00 dir
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 9 {
                    let is_dir = parts[0].starts_with('d');
                    let size: u64 = parts[4].parse().unwrap_or(0);
                    let name = parts[8..].join(" ");
                    if name != "." && name != ".." {
                        let item_path = if resolved.is_empty() {
                            name.clone()
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

        // Read completion response (226)
        resp.clear();
        reader.read_line(&mut resp)?;

        Ok(items)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path);
        let (mut reader, mut writer) = self.connect_control()?;
        let mut data_stream = self.enter_passive_mode(&mut reader, &mut writer)?;

        writer.write_all(format!("STOR {}\r\n", resolved).as_bytes())?;
        writer.flush()?;

        let mut resp = String::new();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::Provider {
                message: format!("FTP STOR initialization failed: {}", resp.trim()),
                status: None,
            });
        }

        let mut file = std::fs::File::open(local_path)?;
        let mut buffer = [0u8; 16384];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            data_stream.write_all(&buffer[..n])?;
        }
        data_stream.flush()?;
        drop(data_stream);

        resp.clear();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("226") && !resp.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP STOR completion failed: {}", resp.trim()),
                status: None,
            });
        }

        info!("Successfully uploaded to FTP: {}", resolved);
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path);
        let (mut reader, mut writer) = self.connect_control()?;
        let mut data_stream = self.enter_passive_mode(&mut reader, &mut writer)?;

        writer.write_all(format!("RETR {}\r\n", resolved).as_bytes())?;
        writer.flush()?;

        let mut resp = String::new();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("150") && !resp.starts_with("125") {
            return Err(StorageError::NotFound(format!(
                "FTP RETR failed for {}: {}",
                resolved,
                resp.trim()
            )));
        }

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(local_path)?;
        let mut buffer = [0u8; 16384];
        loop {
            let n = data_stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n])?;
        }
        file.flush()?;
        drop(data_stream);

        resp.clear();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("226") && !resp.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP RETR completion failed: {}", resp.trim()),
                status: None,
            });
        }

        info!("Successfully downloaded from FTP: {}", resolved);
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path);
        let (mut reader, mut writer) = self.connect_control()?;

        // Try DELE (file) first
        writer.write_all(format!("DELE {}\r\n", resolved).as_bytes())?;
        writer.flush()?;
        let mut resp = String::new();
        reader.read_line(&mut resp)?;

        if resp.starts_with("250") {
            return Ok(());
        }

        // If DELE fails, try RMD (directory)
        writer.write_all(format!("RMD {}\r\n", resolved).as_bytes())?;
        writer.flush()?;
        resp.clear();
        reader.read_line(&mut resp)?;

        if !resp.starts_with("250") {
            return Err(StorageError::NotFound(format!(
                "FTP delete failed for {}: {}",
                resolved,
                resp.trim()
            )));
        }

        Ok(())
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let resolved = self.resolve_remote_path(remote_path);
        let (mut reader, mut writer) = self.connect_control()?;

        writer.write_all(format!("MKD {}\r\n", resolved).as_bytes())?;
        writer.flush()?;
        let mut resp = String::new();
        reader.read_line(&mut resp)?;

        // 257 = "PATHNAME" created, 550 = Directory already exists
        if !resp.starts_with("257") && !resp.starts_with("550") {
            return Err(StorageError::Provider {
                message: format!("FTP MKD failed for {}: {}", resolved, resp.trim()),
                status: None,
            });
        }

        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let resolved_from = self.resolve_remote_path(from);
        let resolved_to = self.resolve_remote_path(to);
        let (mut reader, mut writer) = self.connect_control()?;

        writer.write_all(format!("RNFR {}\r\n", resolved_from).as_bytes())?;
        writer.flush()?;
        let mut resp = String::new();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("350") {
            return Err(StorageError::Provider {
                message: format!("FTP RNFR failed: {}", resp.trim()),
                status: None,
            });
        }

        writer.write_all(format!("RNTO {}\r\n", resolved_to).as_bytes())?;
        writer.flush()?;
        resp.clear();
        reader.read_line(&mut resp)?;
        if !resp.starts_with("250") {
            return Err(StorageError::Provider {
                message: format!("FTP RNTO failed: {}", resp.trim()),
                status: None,
            });
        }

        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("ftp_copy_{}.tmp", rand::random::<u64>()));
        self.download(from, &temp_path).await?;
        let res = self.upload(&temp_path, to).await;
        let _ = tokio::fs::remove_file(&temp_path).await;
        res
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::SERVER_SIDE_MOVE.union(StorageCapabilities::SERVER_SIDE_COPY)
    }
}

/// Builder for [`FtpProvider`].
pub struct FtpProviderBuilder {
    pub credentials: FtpCredentials,
    pub timeout: Option<std::time::Duration>,
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
