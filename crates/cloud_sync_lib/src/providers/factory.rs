use cloud_sync_core::{StorageBackend, StorageError};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

#[allow(dead_code)]
fn config_err(e: impl std::fmt::Display) -> StorageError {
    StorageError::Provider {
        message: format!("Configuration error: {}", e),
        status: None,
    }
}

/// Pluggable factory trait for instantiating storage backends (Open/Closed Principle).
pub trait BackendFactory: Send + Sync {
    /// Returns the unique string identifier for the provider (e.g., "google_drive", "s3", "local").
    fn provider_name(&self) -> &'static str;

    /// Instantiates a thread-safe `StorageBackend` reference from JSON configuration parameters.
    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError>;
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

    /// Looks up and instantiates a backend dynamically by provider name from JSON.
    pub fn create_backend(
        &self,
        provider_name: &str,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        if let Ok(lock) = self.factories.read() {
            if let Some(factory) = lock.get(provider_name) {
                return factory.create(config, sim_root);
            }
        }
        Err(StorageError::NotFound(format!(
            "Provider factory '{}' not registered in DynamicBackendRegistry",
            provider_name
        )))
    }

    /// Returns a new registry pre-populated with standard provider factories based on compiled features.
    pub fn with_defaults() -> Self {
        let registry = Self::new();
        registry.register(Box::new(LocalSimFactory));

        #[cfg(feature = "google_drive")]
        registry.register(Box::new(GoogleDriveFactory));
        #[cfg(feature = "dropbox")]
        registry.register(Box::new(DropboxFactory));
        #[cfg(feature = "onedrive")]
        registry.register(Box::new(OneDriveFactory));
        #[cfg(feature = "webdav")]
        registry.register(Box::new(WebDAVFactory));
        #[cfg(feature = "s3")]
        registry.register(Box::new(S3Factory));
        #[cfg(feature = "box")]
        registry.register(Box::new(BoxFactory));
        #[cfg(feature = "nextcloud")]
        registry.register(Box::new(NextcloudFactory));
        #[cfg(feature = "mega")]
        registry.register(Box::new(MegaFactory));
        #[cfg(feature = "sftp")]
        registry.register(Box::new(SFTPFactory));
        #[cfg(feature = "azure_blob")]
        registry.register(Box::new(AzureBlobFactory));
        #[cfg(feature = "b2")]
        registry.register(Box::new(B2Factory));
        #[cfg(feature = "pcloud")]
        registry.register(Box::new(PCloudFactory));
        #[cfg(feature = "gcs")]
        registry.register(Box::new(GCSFactory));
        #[cfg(feature = "ipfs")]
        registry.register(Box::new(IPFSFactory));

        registry
    }

    /// Returns a shared reference to the global singleton dynamic registry.
    pub fn global() -> &'static DynamicBackendRegistry {
        static INSTANCE: OnceLock<DynamicBackendRegistry> = OnceLock::new();
        INSTANCE.get_or_init(DynamicBackendRegistry::with_defaults)
    }
}

impl Default for DynamicBackendRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ---------------- Built-in Provider Factories ----------------

pub struct LocalSimFactory;

impl BackendFactory for LocalSimFactory {
    fn provider_name(&self) -> &'static str {
        "local"
    }

    fn create(
        &self,
        _config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        Ok(Arc::new(super::local_sim::LocalSimulation::new(
            sim_root,
            "local".to_string(),
        )))
    }
}

#[cfg(feature = "google_drive")]
pub struct GoogleDriveFactory;

#[cfg(feature = "google_drive")]
impl BackendFactory for GoogleDriveFactory {
    fn provider_name(&self) -> &'static str {
        "google_drive"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::OAuthCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::GoogleDrive(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "dropbox")]
pub struct DropboxFactory;

#[cfg(feature = "dropbox")]
impl BackendFactory for DropboxFactory {
    fn provider_name(&self) -> &'static str {
        "dropbox"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::OAuthCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::Dropbox(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "onedrive")]
pub struct OneDriveFactory;

#[cfg(feature = "onedrive")]
impl BackendFactory for OneDriveFactory {
    fn provider_name(&self) -> &'static str {
        "onedrive"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::OAuthCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::OneDrive(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "webdav")]
pub struct WebDAVFactory;

#[cfg(feature = "webdav")]
impl BackendFactory for WebDAVFactory {
    fn provider_name(&self) -> &'static str {
        "webdav"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::WebDAVCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::WebDAV(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "s3")]
pub struct S3Factory;

#[cfg(feature = "s3")]
impl BackendFactory for S3Factory {
    fn provider_name(&self) -> &'static str {
        "s3"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::S3Credentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::S3(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "box")]
pub struct BoxFactory;

#[cfg(feature = "box")]
impl BackendFactory for BoxFactory {
    fn provider_name(&self) -> &'static str {
        "box"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::OAuthCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::Box(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "nextcloud")]
pub struct NextcloudFactory;

#[cfg(feature = "nextcloud")]
impl BackendFactory for NextcloudFactory {
    fn provider_name(&self) -> &'static str {
        "nextcloud"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::NextcloudCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::Nextcloud(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "mega")]
pub struct MegaFactory;

#[cfg(feature = "mega")]
impl BackendFactory for MegaFactory {
    fn provider_name(&self) -> &'static str {
        "mega"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::MegaCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::Mega(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "sftp")]
pub struct SFTPFactory;

#[cfg(feature = "sftp")]
impl BackendFactory for SFTPFactory {
    fn provider_name(&self) -> &'static str {
        "sftp"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::SFTPCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::SFTP(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "azure_blob")]
pub struct AzureBlobFactory;

#[cfg(feature = "azure_blob")]
impl BackendFactory for AzureBlobFactory {
    fn provider_name(&self) -> &'static str {
        "azure_blob"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::AzureBlobCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::AzureBlob(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "b2")]
pub struct B2Factory;

#[cfg(feature = "b2")]
impl BackendFactory for B2Factory {
    fn provider_name(&self) -> &'static str {
        "b2"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::B2Credentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::B2(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "pcloud")]
pub struct PCloudFactory;

#[cfg(feature = "pcloud")]
impl BackendFactory for PCloudFactory {
    fn provider_name(&self) -> &'static str {
        "pcloud"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::PCloudCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::PCloud(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "gcs")]
pub struct GCSFactory;

#[cfg(feature = "gcs")]
impl BackendFactory for GCSFactory {
    fn provider_name(&self) -> &'static str {
        "gcs"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::GCSCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::GCS(creds),
            sim_root,
        ))
    }
}

#[cfg(feature = "ipfs")]
pub struct IPFSFactory;

#[cfg(feature = "ipfs")]
impl BackendFactory for IPFSFactory {
    fn provider_name(&self) -> &'static str {
        "ipfs"
    }

    fn create(
        &self,
        config: &JsonValue,
        sim_root: PathBuf,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let creds: super::IPFSCredentials =
            serde_json::from_value(config.clone()).map_err(config_err)?;
        Ok(super::BackendRegistry::create_backend(
            super::BackendCredentials::IPFS(creds),
            sim_root,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CustomTestFactory;

    impl BackendFactory for CustomTestFactory {
        fn provider_name(&self) -> &'static str {
            "custom_mock"
        }

        fn create(
            &self,
            _config: &JsonValue,
            sim_root: PathBuf,
        ) -> Result<Arc<dyn StorageBackend>, StorageError> {
            Ok(Arc::new(super::super::local_sim::LocalSimulation::new(
                sim_root,
                "custom_mock".to_string(),
            )))
        }
    }

    #[test]
    fn test_dynamic_registry_registration_and_build() {
        let registry = DynamicBackendRegistry::new();
        registry.register(Box::new(CustomTestFactory));

        let res = registry.create_backend(
            "custom_mock",
            &serde_json::json!({}),
            PathBuf::from("dummy_dir"),
        );
        assert!(res.is_ok());
        let backend = res.unwrap();
        assert_eq!(backend.name(), "custom_mock");

        let unknown = registry.create_backend(
            "nonexistent",
            &serde_json::json!({}),
            PathBuf::from("dummy_dir"),
        );
        assert!(unknown.is_err());
    }

    #[test]
    fn test_dynamic_registry_default_providers() {
        let registry = DynamicBackendRegistry::with_defaults();
        let res = registry.create_backend(
            "local",
            &serde_json::json!({}),
            PathBuf::from("sim_dir"),
        );
        assert!(res.is_ok());
    }
}
