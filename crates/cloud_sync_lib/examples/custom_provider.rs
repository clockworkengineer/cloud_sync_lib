//! # Custom Provider Registration Example
//!
//! Demonstrates how to create a custom storage provider adhering to SOLID principles
//! and register it into `DynamicBackendRegistry` without touching any library internals.
//!
//! Run with:
//! ```bash
//! cargo run --example custom_provider -p cloud_sync_lib
//! ```

use async_trait::async_trait;
use cloud_sync_lib::{
    BackendFactory, CloudSyncClient, DynamicBackendRegistry, StorageBackend,
    StorageCapabilities, StorageError, StorageItem,
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// An in-memory key-value storage backend implementation.
pub struct InMemoryBackend {
    name: String,
    files: RwLock<HashMap<String, Vec<u8>>>,
}

impl InMemoryBackend {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            files: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl StorageBackend for InMemoryBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::RESUMABLE_UPLOAD
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let bytes = tokio::fs::read(local_path)
            .await
            .map_err(StorageError::Io)?;
        {
            let mut lock = self.files.write().unwrap();
            lock.insert(remote_path.to_string(), bytes);
            println!("[InMemoryBackend] Stored {} bytes at '{}'", lock.get(remote_path).unwrap().len(), remote_path);
        }
        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let maybe_bytes = {
            let lock = self.files.read().unwrap();
            lock.get(remote_path).cloned()
        };

        if let Some(bytes) = maybe_bytes {
            tokio::fs::write(local_path, bytes)
                .await
                .map_err(StorageError::Io)?;
            println!("[InMemoryBackend] Downloaded '{}' -> {:?}", remote_path, local_path);
            Ok(())
        } else {
            Err(StorageError::NotFound(format!("File '{}' not in memory", remote_path)))
        }
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let mut lock = self.files.write().unwrap();
        lock.remove(remote_path);
        Ok(())
    }

    async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let lock = self.files.read().unwrap();
        let items = lock
            .iter()
            .map(|(path, data)| StorageItem {
                path: PathBuf::from(path),
                size: data.len() as u64,
                modified: std::time::SystemTime::now(),
                is_dir: false,
                checksum: None,
                permissions: None,
            })
            .collect();
        Ok(items)
    }
}

/// Factory for building InMemoryBackend instances from JSON configuration.
pub struct InMemoryFactory;

impl BackendFactory for InMemoryFactory {
    fn provider_name(&self) -> &'static str {
        "in_memory"
    }

    fn create(
        &self,
        config: &JsonValue,
        _sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let name = config["name"].as_str().unwrap_or("in_memory_default");
        Ok(Arc::new(InMemoryBackend::new(name)))
    }
}

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    println!("=== CloudSync: Custom Provider Example ===");

    // 1. Create a dynamic backend registry and register the custom factory (OCP)
    let registry = Arc::new(DynamicBackendRegistry::new());
    registry.register(Box::new(InMemoryFactory));
    println!("Registered custom 'in_memory' backend factory.");

    // 2. Build client using the custom factory and JSON config (Attribute 1 & 6)
    let config = serde_json::json!({ "name": "ephemeral_vault" });
    let client = CloudSyncClient::builder()
        .with_registry(registry)
        .with_provider("in_memory", config, PathBuf::from("/tmp"))
        .with_logging()
        .build()?;

    println!("Created client with provider: {}", client.provider_name());

    // 3. Test saving and reading a file
    let temp_dir = tempfile::tempdir().map_err(StorageError::Io)?;
    let test_file = temp_dir.path().join("secret.key");
    tokio::fs::write(&test_file, b"super-secret-key-material")
        .await
        .map_err(StorageError::Io)?;

    client.upload(&test_file, "keys/secret.key").await?;

    let list = client.list("keys").await?;
    println!("Items in custom vault: {}", list.len());

    let restored_file = temp_dir.path().join("recovered.key");
    client.download("keys/secret.key", &restored_file).await?;

    let content = tokio::fs::read(&restored_file)
        .await
        .map_err(StorageError::Io)?;
    println!("Successfully verified custom provider data: {} bytes", content.len());

    println!("=== Custom Provider Example completed! ===");
    Ok(())
}

