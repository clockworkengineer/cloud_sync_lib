use serde::{Deserialize, Serialize};
use std::path::Path;
use cloud_sync_lib::{ProviderRootsConfig, ProviderCredentialsConfig};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupSection {
    pub source_provider: String,
    pub source_path: Option<String>,
    pub destination_provider: String,
    pub destination_path: Option<String>,
    pub backup_interval_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupConfig {
    pub backup: BackupSection,
    #[serde(flatten)]
    pub roots: ProviderRootsConfig,
    #[serde(flatten)]
    pub credentials: ProviderCredentialsConfig,
    pub exclude: Option<Vec<String>>,
    pub max_upload_rate: Option<u64>,
    pub max_download_rate: Option<u64>,
    pub max_concurrency: Option<usize>,
}

impl std::ops::Deref for BackupConfig {
    type Target = ProviderCredentialsConfig;
    fn deref(&self) -> &Self::Target {
        &self.credentials
    }
}

pub async fn load_config(path: &str) -> Result<BackupConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
    let content = tokio::fs::read_to_string(config_path).await?;
    let config: BackupConfig = toml::from_str(&content)?;
    Ok(config)
}
