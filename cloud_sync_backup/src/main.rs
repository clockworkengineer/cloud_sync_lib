use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;
use cloud_sync_lib::{StorageBackend, StorageItem, LocalSimulation, SimulatedFallback};
use cloud_sync_lib::{GoogleDriveProvider, DropboxProvider, OneDriveProvider, WebDAVProvider, S3Provider, SFTPProvider, NextcloudProvider, MegaProvider};

mod config;
use config::{BackupConfig, load_config};

fn build_backend(provider_name: &str, custom_path: Option<&str>, config: &BackupConfig) -> Result<Arc<dyn StorageBackend>, Box<dyn std::error::Error>> {
    let name_lower = provider_name.to_lowercase();
    match name_lower.as_str() {
        "local" => {
            let path_str = custom_path.ok_or("Local provider requires source_path or destination_path configured")?;
            let path = PathBuf::from(path_str);
            Ok(Arc::new(LocalSimulation::new(path, "Local".to_string())))
        }
        "google_drive" | "google drive" => {
            let creds = config.google_credentials.clone().ok_or("Google credentials not configured")?;
            let root = config.google_drive_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/google_drive"));
            let inner = Arc::new(GoogleDriveProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "Google Drive".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "Google Drive", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "dropbox" => {
            let creds = config.dropbox_credentials.clone().ok_or("Dropbox credentials not configured")?;
            let root = config.dropbox_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/dropbox"));
            let inner = Arc::new(DropboxProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "Dropbox".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "Dropbox", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "onedrive" => {
            let creds = config.onedrive_credentials.clone().ok_or("OneDrive credentials not configured")?;
            let root = config.onedrive_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/onedrive"));
            let inner = Arc::new(OneDriveProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "OneDrive".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "OneDrive", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "webdav" => {
            let creds = config.webdav_credentials.clone().ok_or("WebDAV credentials not configured")?;
            let root = config.webdav_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/webdav"));
            let inner = Arc::new(WebDAVProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "WebDAV".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "WebDAV", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "s3" => {
            let creds = config.s3_credentials.clone().ok_or("S3 credentials not configured")?;
            let root = config.s3_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/s3"));
            let inner = Arc::new(S3Provider::new(creds));
            let local_sim = LocalSimulation::new(root, "S3".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "S3", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "sftp" => {
            let creds = config.sftp_credentials.clone().ok_or("SFTP credentials not configured")?;
            let root = config.sftp_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/sftp"));
            let inner = Arc::new(SFTPProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "SFTP".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "SFTP", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "nextcloud" => {
            let creds = config.nextcloud_credentials.clone().ok_or("Nextcloud credentials not configured")?;
            let root = config.nextcloud_root.clone().unwrap_or_else(|| PathBuf::from("./cloud_simulation/nextcloud"));
            let inner = Arc::new(NextcloudProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "Nextcloud".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "Nextcloud", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        "mega" => {
            let creds = config.mega_credentials.clone().ok_or("MEGA credentials not configured")?;
            let root = config.mega_root.clone().unwrap_or_else(|| PathBuf::from("mega_backup"));
            let inner = Arc::new(MegaProvider::new(creds));
            let local_sim = LocalSimulation::new(root, "MEGA".to_string());
            let fallback = SimulatedFallback::new(Some(inner), local_sim, "MEGA", cloud_sync_lib::SyncMode::OneWay);
            Ok(Arc::new(fallback))
        }
        _ => Err(format!("Unsupported backup provider: {}", provider_name).into()),
    }
}

async fn scan_backend_files(backend: &dyn StorageBackend) -> Result<HashMap<String, StorageItem>, Box<dyn std::error::Error>> {
    let mut files = HashMap::new();
    let mut queue = vec!["".to_string()];

    while let Some(current) = queue.pop() {
        match backend.list(&current).await {
            Ok(items) => {
                for item in items {
                    let path_str = item.path.to_string_lossy().to_string();
                    if item.is_dir {
                        queue.push(path_str.clone());
                    }
                    files.insert(path_str, item);
                }
            }
            Err(cloud_sync_lib::StorageError::NotFound(_)) => {}
            Err(e) => return Err(Box::new(e)),
        }
    }
    Ok(files)
}

async fn perform_backup(
    source: &dyn StorageBackend,
    destination: &dyn StorageBackend,
    synced_history: &mut HashMap<String, (u64, std::time::SystemTime, Option<String>)>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let source_files = scan_backend_files(source).await?;
    let dest_files = scan_backend_files(destination).await?;

    let temp_dir = std::env::temp_dir().join("cloud_sync_backup_temp");
    tokio::fs::create_dir_all(&temp_dir).await?;

    let same_provider = source.name() == destination.name();
    let mut sync_count = 0;

    for (rel_path, source_item) in source_files {
        let should_copy = match dest_files.get(&rel_path) {
            Some(dest_item) => {
                if let Some((last_size, last_modified, last_checksum)) = synced_history.get(&rel_path) {
                    if source_item.size == *last_size
                        && source_item.modified == *last_modified
                        && source_item.checksum == *last_checksum
                    {
                        false
                    } else {
                        true
                    }
                } else {
                    let s_secs = source_item.modified.duration_since(std::time::SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let d_secs = dest_item.modified.duration_since(std::time::SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    if same_provider {
                        if let (Some(s_sum), Some(d_sum)) = (&source_item.checksum, &dest_item.checksum) {
                            s_sum != d_sum
                        } else {
                            source_item.size != dest_item.size || s_secs > d_secs
                        }
                    } else {
                        source_item.size != dest_item.size
                    }
                }
            }
            None => true,
        };

        if should_copy {
            if source_item.is_dir {
                println!("[Backup] Creating remote directory: {}", rel_path);
                destination.create_folder(&rel_path).await?;
            } else {
                println!("[Backup] Syncing file: {}", rel_path);
                let local_temp = temp_dir.join(&rel_path);
                if let Some(parent) = local_temp.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                source.download(&rel_path, &local_temp).await?;
                let ft = filetime::FileTime::from_system_time(source_item.modified);
                let _ = filetime::set_file_mtime(&local_temp, ft);

                destination.upload(&local_temp, &rel_path).await?;

                let _ = tokio::fs::remove_file(&local_temp).await;
                sync_count += 1;

                synced_history.insert(
                    rel_path.clone(),
                    (source_item.size, source_item.modified, source_item.checksum.clone()),
                );
            }
        } else {
            synced_history.insert(
                rel_path.clone(),
                (source_item.size, source_item.modified, source_item.checksum.clone()),
            );
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Ok(sync_count)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let config_file = if args.len() > 1 {
        &args[1]
    } else {
        "backup_config.toml"
    };

    println!("[Backup] Loading config: {}...", config_file);
    let config = load_config(config_file).await?;

    let source = build_backend(
        &config.backup.source_provider,
        config.backup.source_path.as_deref(),
        &config,
    )?;

    let destination = build_backend(
        &config.backup.destination_provider,
        config.backup.destination_path.as_deref(),
        &config,
    )?;

    let interval = config.backup.backup_interval_secs.unwrap_or(60);
    println!(
        "[Backup] Initializing backup loop (Source: {} -> Destination: {}) every {} seconds",
        source.name(),
        destination.name(),
        interval
    );

    let mut synced_history = HashMap::new();
    loop {
        match perform_backup(&*source, &*destination, &mut synced_history).await {
            Ok(count) => {
                if count > 0 {
                    println!("[Backup] Backup scan completed. Synced {} item(s).", count);
                }
            }
            Err(e) => eprintln!("[Backup] Backup scan failed: {}", e),
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }
}
