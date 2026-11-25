use cloud_sync_lib::{BackendCredentials, LocalSimulation, StorageBackend, StorageItem};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

mod config;
use config::{load_config, BackupConfig};

fn build_backend(
    provider_name: &str,
    custom_path: Option<&str>,
    config: &BackupConfig,
) -> Result<Arc<dyn StorageBackend>, Box<dyn std::error::Error>> {
    let name_lower = provider_name.to_lowercase();
    match name_lower.as_str() {
        "local" => {
            let path_str = custom_path
                .ok_or("Local provider requires source_path or destination_path configured")?;
            let path = PathBuf::from(path_str);
            Ok(Arc::new(LocalSimulation::new(path, "Local".to_string())))
        }
        _ => {
            let (creds, root) = match name_lower.as_str() {
                #[cfg(feature = "google_drive")]
                "google_drive" | "google drive" => {
                    let creds = config
                        .google_credentials
                        .clone()
                        .ok_or("Google credentials not configured")?;
                    let root = config
                        .roots
                        .google_drive_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/google_drive"));
                    (BackendCredentials::GoogleDrive(creds), root)
                }
                #[cfg(feature = "dropbox")]
                "dropbox" => {
                    let creds = config
                        .dropbox_credentials
                        .clone()
                        .ok_or("Dropbox credentials not configured")?;
                    let root = config
                        .roots
                        .dropbox_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/dropbox"));
                    (BackendCredentials::Dropbox(creds), root)
                }
                #[cfg(feature = "onedrive")]
                "onedrive" => {
                    let creds = config
                        .onedrive_credentials
                        .clone()
                        .ok_or("OneDrive credentials not configured")?;
                    let root = config
                        .roots
                        .onedrive_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/onedrive"));
                    (BackendCredentials::OneDrive(creds), root)
                }
                #[cfg(feature = "webdav")]
                "webdav" => {
                    let creds = config
                        .webdav_credentials
                        .clone()
                        .ok_or("WebDAV credentials not configured")?;
                    let root = config
                        .roots
                        .webdav_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/webdav"));
                    (BackendCredentials::WebDAV(creds), root)
                }
                #[cfg(feature = "s3")]
                "s3" => {
                    let creds = config
                        .s3_credentials
                        .clone()
                        .ok_or("S3 credentials not configured")?;
                    let root = config
                        .roots
                        .s3_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/s3"));
                    (BackendCredentials::S3(creds), root)
                }
                #[cfg(feature = "sftp")]
                "sftp" => {
                    let creds = config
                        .sftp_credentials
                        .clone()
                        .ok_or("SFTP credentials not configured")?;
                    let root = config
                        .roots
                        .sftp_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/sftp"));
                    (BackendCredentials::SFTP(creds), root)
                }
                #[cfg(feature = "nextcloud")]
                "nextcloud" => {
                    let creds = config
                        .nextcloud_credentials
                        .clone()
                        .ok_or("Nextcloud credentials not configured")?;
                    let root = config
                        .roots
                        .nextcloud_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/nextcloud"));
                    (BackendCredentials::Nextcloud(creds), root)
                }
                #[cfg(feature = "mega")]
                "mega" => {
                    let creds = config
                        .mega_credentials
                        .clone()
                        .ok_or("MEGA credentials not configured")?;
                    let root = config
                        .roots
                        .mega_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("mega_backup"));
                    (BackendCredentials::Mega(creds), root)
                }
                #[cfg(feature = "box")]
                "box" => {
                    let creds = config
                        .box_credentials
                        .clone()
                        .ok_or("Box credentials not configured")?;
                    let root = config
                        .roots
                        .box_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/box"));
                    (BackendCredentials::Box(creds), root)
                }
                #[cfg(feature = "azure_blob")]
                "azure_blob" | "azure" => {
                    let creds = config
                        .azure_blob_credentials
                        .clone()
                        .ok_or("Azure Blob credentials not configured")?;
                    let root = config
                        .roots
                        .azure_blob_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/azure_blob"));
                    (BackendCredentials::AzureBlob(creds), root)
                }
                #[cfg(feature = "gcs")]
                "gcs" => {
                    let creds = config
                        .gcs_credentials
                        .clone()
                        .ok_or("GCS credentials not configured")?;
                    let root = config
                        .roots
                        .gcs_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/gcs"));
                    (BackendCredentials::GCS(creds), root)
                }
                #[cfg(feature = "b2")]
                "b2" | "backblaze" => {
                    let creds = config
                        .b2_credentials
                        .clone()
                        .ok_or("Backblaze B2 credentials not configured")?;
                    let root = config
                        .roots
                        .b2_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/b2"));
                    (BackendCredentials::B2(creds), root)
                }
                #[cfg(feature = "pcloud")]
                "pcloud" => {
                    let creds = config
                        .pcloud_credentials
                        .clone()
                        .ok_or("pCloud credentials not configured")?;
                    let root = config
                        .roots
                        .pcloud_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/pcloud"));
                    (BackendCredentials::PCloud(creds), root)
                }
                #[cfg(feature = "ipfs")]
                "ipfs" => {
                    let creds = config
                        .ipfs_credentials
                        .clone()
                        .ok_or("IPFS credentials not configured")?;
                    let root = config
                        .roots
                        .ipfs_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/ipfs"));
                    (BackendCredentials::IPFS(creds), root)
                }
                #[cfg(feature = "ftp")]
                "ftp" | "ftps" => {
                    let creds = config
                        .ftp_credentials
                        .clone()
                        .ok_or("FTP credentials not configured")?;
                    let root = config
                        .roots
                        .ftp_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/ftp"));
                    (BackendCredentials::FTP(creds), root)
                }
                #[cfg(feature = "http")]
                "http" | "https" => {
                    let creds = config
                        .http_credentials
                        .clone()
                        .ok_or("HTTP credentials not configured")?;
                    let root = config
                        .roots
                        .http_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/http"));
                    (BackendCredentials::HTTP(creds), root)
                }
                #[cfg(feature = "smb")]
                "smb" | "cifs" => {
                    let creds = config
                        .smb_credentials
                        .clone()
                        .ok_or("SMB credentials not configured")?;
                    let root = config
                        .roots
                        .smb_root
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("./cloud_simulation/smb"));
                    (BackendCredentials::SMB(creds), root)
                }
                _ => {
                    return Err(format!(
                        "Unsupported backup provider or disabled feature: {}",
                        provider_name
                    )
                    .into())
                }
            };
            Ok(cloud_sync_lib::create_backend(creds, root))
        }
    }
}

async fn scan_backend_files<B: StorageBackend + ?Sized>(
    backend: &B,
) -> Result<HashMap<String, StorageItem>, Box<dyn std::error::Error>> {
    let mut files = HashMap::new();
    let mut queue = vec![("".to_string(), 0usize)];
    let mut visited = std::collections::HashSet::new();

    while let Some((current, depth)) = queue.pop() {
        if depth > 64 || !visited.insert(current.clone()) {
            continue;
        }
        match backend.list(&current).await {
            Ok(items) => {
                for item in items {
                    let path_str = item.path.to_string_lossy().to_string().replace('\\', "/");
                    if item.is_dir {
                        queue.push((path_str.clone(), depth + 1));
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

async fn perform_backup<S: StorageBackend + ?Sized, D: StorageBackend + ?Sized>(
    source: &S,
    destination: &D,
    synced_history: &mut HashMap<String, (u64, std::time::SystemTime, Option<String>)>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let source_files = scan_backend_files(source).await?;
    let dest_files = scan_backend_files(destination).await?;

    let temp_dir = std::env::temp_dir().join("cloud_sync_backup_temp");
    tokio::fs::create_dir_all(&temp_dir).await?;

    let same_provider = source.name() == destination.name();
    let mut sync_count = 0;

    let mut dest_files_lower = HashMap::new();
    for (path, item) in &dest_files {
        dest_files_lower.insert(path.to_lowercase(), item);
    }

    for (rel_path, source_item) in source_files {
        let rel_path_lower = rel_path.to_lowercase();
        let should_copy = match dest_files_lower.get(&rel_path_lower) {
            Some(dest_item) => {
                if source_item.is_dir {
                    false
                } else if let Some((last_size, last_modified, last_checksum)) =
                    synced_history.get(&rel_path)
                {
                    source_item.size != *last_size
                        || source_item.modified != *last_modified
                        || source_item.checksum != *last_checksum
                } else {
                    let s_secs = source_item
                        .modified
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let d_secs = dest_item
                        .modified
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if same_provider {
                        if let (Some(s_sum), Some(d_sum)) =
                            (&source_item.checksum, &dest_item.checksum)
                        {
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
                sync_count += 1;
                synced_history.insert(
                    rel_path.clone(),
                    (
                        source_item.size,
                        source_item.modified,
                        source_item.checksum.clone(),
                    ),
                );
            } else {
                println!("[Backup] Syncing file: {}", rel_path);

                // Use RAII-guarded temporary staging file to guarantee cleanup even if download or upload fails
                let temp_guard =
                    cloud_sync_lib::providers::utils::TempFileGuard::new("backup_transfer");
                source.download(&rel_path, temp_guard.path()).await?;
                let ft = filetime::FileTime::from_system_time(source_item.modified);
                let _ = filetime::set_file_mtime(temp_guard.path(), ft);

                destination.upload(temp_guard.path(), &rel_path).await?;
                sync_count += 1;

                synced_history.insert(
                    rel_path.clone(),
                    (
                        source_item.size,
                        source_item.modified,
                        source_item.checksum.clone(),
                    ),
                );
            }
        } else {
            synced_history.insert(
                rel_path.clone(),
                (
                    source_item.size,
                    source_item.modified,
                    source_item.checksum.clone(),
                ),
            );
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Ok(sync_count)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<String> = std::env::args().collect();
    let mut log_file_path = None;
    let mut run_once = false;
    let mut snapshot_label: Option<String> = None;
    let mut list_snaps = false;
    let mut args = Vec::new();
    if !raw_args.is_empty() {
        args.push(raw_args[0].clone());
    }
    let mut i = 1;
    while i < raw_args.len() {
        if (raw_args[i] == "--log-file" || raw_args[i] == "--log") && i + 1 < raw_args.len() {
            log_file_path = Some(raw_args[i + 1].clone());
            i += 2;
        } else if raw_args[i] == "--once" {
            run_once = true;
            i += 1;
        } else if raw_args[i] == "--list-snapshots" {
            list_snaps = true;
            i += 1;
        } else if raw_args[i] == "--snapshot" {
            if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with("--") {
                snapshot_label = Some(raw_args[i + 1].clone());
                i += 2;
            } else {
                snapshot_label = Some("cli".to_string());
                i += 1;
            }
        } else if raw_args[i].ends_with(".log") {
            log_file_path = Some(raw_args[i].clone());
            i += 1;
        } else {
            args.push(raw_args[i].clone());
            i += 1;
        }
    }

    let mut _log_guard = None;
    if let Some(path) = log_file_path {
        let path = std::path::PathBuf::from(path);
        let directory = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let file_name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("backup.log"))
            .to_string_lossy()
            .to_string();
        let file_appender = tracing_appender::rolling::daily(directory, file_name);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _log_guard = Some(guard);

        tracing_subscriber::fmt().with_writer(non_blocking).init();
    } else {
        tracing_subscriber::fmt::init();
    }

    let resolved_config = if args.len() > 1 {
        args[1].clone()
    } else {
        let local_path = "backup_config.toml";
        if std::path::Path::new(local_path).exists() {
            local_path.to_string()
        } else {
            config::get_default_backup_config_path()
                .to_string_lossy()
                .to_string()
        }
    };

    println!("[Backup] Loading config: {}...", resolved_config);
    let config = load_config(&resolved_config).await?;

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

    if list_snaps {
        let snaps = cloud_sync_lib::list_snapshots(&*destination).await?;
        println!(
            "[Backup] Found {} snapshot(s) on {}:",
            snaps.len(),
            destination.name()
        );
        for s in snaps {
            println!(
                "  - {} (label: '{}', files: {}, total: {} B, saved by dedup: {} B)",
                s.id, s.label, s.total_files, s.total_bytes, s.bytes_saved_by_dedup
            );
        }
        return Ok(());
    }

    if let Some(label) = snapshot_label {
        let local_src = config
            .backup
            .source_path
            .as_deref()
            .ok_or("--snapshot requires backup.source_path pointing to a local directory")?;
        let mf =
            cloud_sync_lib::create_snapshot(&*destination, std::path::Path::new(local_src), &label)
                .await?;
        println!(
            "[Backup] Created deduplicated snapshot '{}' on {} ({} files, {} new blobs, {} reused blobs)",
            mf.id,
            destination.name(),
            mf.total_files,
            mf.new_blobs_uploaded,
            mf.deduped_blobs_reused
        );
        return Ok(());
    }

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
        if run_once {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_build_backend_local_and_error() {
        let temp = tempdir().unwrap();
        let cfg = BackupConfig {
            backup: config::BackupSection {
                source_provider: "local".into(),
                source_path: Some(temp.path().to_string_lossy().to_string()),
                destination_provider: "local".into(),
                destination_path: Some(temp.path().to_string_lossy().to_string()),
                backup_interval_secs: Some(30),
            },
            roots: Default::default(),
            credentials: Default::default(),
            exclude: None,
            max_upload_rate: None,
            max_download_rate: None,
            max_concurrency: None,
        };

        // Local provider with valid path
        let b1 = build_backend("local", Some(temp.path().to_str().unwrap()), &cfg);
        assert!(b1.is_ok());

        // Local provider without path
        let b2 = build_backend("local", None, &cfg);
        assert!(b2.is_err());

        // Unknown provider
        let b3 = build_backend("unsupported_cloud_xyz", None, &cfg);
        assert!(b3.is_err());
    }

    #[tokio::test]
    async fn test_scan_and_perform_backup() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("source");
        let dst_dir = temp.path().join("destination");

        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();

        let sub = src_dir.join("subfolder");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("file1.txt"), b"backup test content 1").unwrap();
        std::fs::write(src_dir.join("file2.txt"), b"backup test content 2").unwrap();

        let source = LocalSimulation::new(src_dir, "Source".to_string());
        let destination = LocalSimulation::new(dst_dir.clone(), "Destination".to_string());

        let mut history = HashMap::new();

        // Initial backup
        let count = perform_backup(&source, &destination, &mut history)
            .await
            .expect("backup succeeded");
        assert!(count >= 2);

        // Files exist at destination
        assert!(dst_dir.join("subfolder").join("file1.txt").exists());
        assert!(dst_dir.join("file2.txt").exists());

        // Second pass should copy 0 files
        let count2 = perform_backup(&source, &destination, &mut history)
            .await
            .expect("backup rerun succeeded");
        assert_eq!(count2, 0);
    }
}
