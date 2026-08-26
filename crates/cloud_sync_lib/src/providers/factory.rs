use cloud_sync_core::{StorageBackend, StorageError};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Pluggable factory trait for instantiating storage backends (Open/Closed Principle).
pub trait BackendFactory: Send + Sync {
    /// Returns the unique string identifier for the provider (e.g., "google_drive", "s3", "local").
    fn provider_name(&self) -> &'static str;

    /// Instantiates a thread-safe `StorageBackend` reference from JSON/TOML configuration parameters.
    fn create(&self, config: &JsonValue, sim_root: PathBuf) -> Result<Arc<dyn StorageBackend>, StorageError>;
}

/// Thread-safe dynamic registry allowing self-registration of storage backend factories.
pub struct DynamicBackendRegistry {
    factories: RwLock<HashMap<&'static str, Box<dyn BackendFactory>>>,
}

impl DynamicBackendRegistry {
    pub fn new() -> Self {
        Self {
            factories: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a new provider factory into the dynamic registry without modifying central core files.
    pub fn register(&self, factory: Box<dyn BackendFactory>) {
        if let Ok(mut lock) = self.factories.write() {
            lock.insert(factory.provider_name(), factory);
        }
    }

    /// Looks up and instantiates a backend dynamically by provider name.
    pub fn create_backend(&self, provider_name: &str, config: &JsonValue, sim_root: PathBuf) -> Result<Arc<dyn StorageBackend>, StorageError> {
        if let Ok(lock) = self.factories.read() {
            if let Some(factory) = lock.get(provider_name) {
                return factory.create(config, sim_root);
            }
        }
        Err(StorageError::NotFound(format!("Provider factory '{}' not registered in DynamicBackendRegistry", provider_name)))
    }
}

impl Default for DynamicBackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}
