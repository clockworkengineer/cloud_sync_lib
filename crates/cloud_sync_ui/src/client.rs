//! Daemon client gateway port and transport adapters (DIP).

use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
pub enum DaemonClientError {
    ConnectionFailed(String),
    Io(std::io::Error),
    Serialization(String),
    CommandFailed(String),
}

impl fmt::Display for DaemonClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "Daemon connection failed: {}", msg),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            Self::CommandFailed(msg) => write!(f, "Command execution failed: {}", msg),
        }
    }
}

impl std::error::Error for DaemonClientError {}

impl From<std::io::Error> for DaemonClientError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Abstract gateway port for communicating with the sync daemon (DIP).
#[async_trait]
pub trait DaemonClientPort: Send + Sync {
    /// Sends a raw string command line to the daemon and returns the response.
    async fn send_command(&self, cmd: &str) -> Result<String, DaemonClientError>;

    /// Fetches the parsed daemon status JSON.
    async fn get_status(&self) -> Result<serde_json::Value, DaemonClientError> {
        let raw = self.send_command("status").await?;
        let parsed = crate::parser::parse_status(&raw);
        Ok(serde_json::to_value(parsed)
            .map_err(|e| DaemonClientError::Serialization(e.to_string()))?)
    }

    /// Fetches the current synchronization plan.
    async fn get_plan(&self) -> Result<serde_json::Value, DaemonClientError> {
        let raw = self.send_command("get_plan").await?;
        serde_json::from_str(&raw).map_err(|e| DaemonClientError::Serialization(e.to_string()))
    }

    /// Triggers an immediate bidirectional synchronization cycle.
    async fn trigger_sync(&self) -> Result<String, DaemonClientError> {
        self.send_command("sync").await
    }

    /// Pauses file watching and scheduled sync.
    async fn pause(&self) -> Result<String, DaemonClientError> {
        self.send_command("pause").await
    }

    /// Resumes file watching and scheduled sync.
    async fn resume(&self) -> Result<String, DaemonClientError> {
        self.send_command("resume").await
    }

    /// Resolves a file conflict with a choice ("local" or "remote").
    async fn resolve_conflict(
        &self,
        rel_path: &str,
        choice: &str,
    ) -> Result<String, DaemonClientError> {
        self.send_command(&format!("resolve_conflict {} {}", rel_path, choice))
            .await
    }
}

/// Standard production client communicating with daemon over a TCP socket.
#[derive(Clone, Debug)]
pub struct TcpDaemonClient {
    addr: String,
}

impl TcpDaemonClient {
    pub fn new(addr: impl Into<String>) -> Self {
        Self { addr: addr.into() }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }
}

#[async_trait]
impl DaemonClientPort for TcpDaemonClient {
    async fn send_command(&self, cmd: &str) -> Result<String, DaemonClientError> {
        let mut stream = TcpStream::connect(&self.addr)
            .await
            .map_err(|e| DaemonClientError::ConnectionFailed(e.to_string()))?;
        stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
        stream.flush().await?;

        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        Ok(response)
    }
}

#[async_trait]
impl<T: DaemonClientPort + ?Sized> DaemonClientPort for Arc<T> {
    async fn send_command(&self, cmd: &str) -> Result<String, DaemonClientError> {
        (**self).send_command(cmd).await
    }
    async fn get_status(&self) -> Result<serde_json::Value, DaemonClientError> {
        (**self).get_status().await
    }
    async fn get_plan(&self) -> Result<serde_json::Value, DaemonClientError> {
        (**self).get_plan().await
    }
    async fn trigger_sync(&self) -> Result<String, DaemonClientError> {
        (**self).trigger_sync().await
    }
    async fn pause(&self) -> Result<String, DaemonClientError> {
        (**self).pause().await
    }
    async fn resume(&self) -> Result<String, DaemonClientError> {
        (**self).resume().await
    }
    async fn resolve_conflict(
        &self,
        rel_path: &str,
        choice: &str,
    ) -> Result<String, DaemonClientError> {
        (**self).resolve_conflict(rel_path, choice).await
    }
}

/// Mock command handler closure type.
pub type MockCommandHandler = Box<dyn Fn(&str) -> Result<String, DaemonClientError> + Send + Sync>;

/// Mock daemon client for offline presentation and integration testing without sockets.
pub struct MockDaemonClient {
    handler: MockCommandHandler,
}

impl MockDaemonClient {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&str) -> Result<String, DaemonClientError> + Send + Sync + 'static,
    {
        Self {
            handler: Box::new(handler),
        }
    }
}

#[async_trait]
impl DaemonClientPort for MockDaemonClient {
    async fn send_command(&self, cmd: &str) -> Result<String, DaemonClientError> {
        (self.handler)(cmd)
    }
}
