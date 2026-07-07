//! Standalone diagnostic utility to verify the status of all configured backends.

#[path = "../config.rs"]
pub mod config;

#[path = "common.rs"]
pub mod common;

use cloud_sync_lib::StorageBackend;
use cloud_sync_lib::providers::*;

async fn check_provider(name: &str, provider: &dyn StorageBackend) {
    print!("{}: ", name);
    match provider.list("").await {
        Ok(_) => {
            println!("YES");
        }
        Err(e) => {
            println!("NO ({:?})", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_file = common::resolve_config_file();
    let config = config::load_or_create_config(config_file).await?;

    println!("Checking status of configured backends...\n");

    #[cfg(feature = "google_drive")]
    if let Some(creds) = &config.google_credentials {
        if config::is_provider_enabled(&config.google_credentials) {
            let provider = GoogleDriveProvider::new(creds.clone());
            check_provider("Google Drive", &provider).await;
        }
    }

    #[cfg(feature = "dropbox")]
    if let Some(creds) = &config.dropbox_credentials {
        if config::is_provider_enabled(&config.dropbox_credentials) {
            let provider = DropboxProvider::new(creds.clone());
            check_provider("Dropbox", &provider).await;
        }
    }

    #[cfg(feature = "onedrive")]
    if let Some(creds) = &config.onedrive_credentials {
        if config::is_provider_enabled(&config.onedrive_credentials) {
            let provider = OneDriveProvider::new(creds.clone());
            check_provider("OneDrive", &provider).await;
        }
    }

    #[cfg(feature = "webdav")]
    if let Some(creds) = &config.webdav_credentials {
        if config::is_provider_enabled(&config.webdav_credentials) {
            let provider = WebDAVProvider::new(creds.clone());
            check_provider("WebDAV", &provider).await;
        }
    }

    #[cfg(feature = "s3")]
    if let Some(creds) = &config.s3_credentials {
        if config::is_provider_enabled(&config.s3_credentials) {
            let provider = S3Provider::new(creds.clone());
            check_provider("S3", &provider).await;
        }
    }

    #[cfg(feature = "sftp")]
    if let Some(creds) = &config.sftp_credentials {
        if config::is_provider_enabled(&config.sftp_credentials) {
            let provider = SFTPProvider::new(creds.clone());
            check_provider("SFTP", &provider).await;
        }
    }

    #[cfg(feature = "nextcloud")]
    if let Some(creds) = &config.nextcloud_credentials {
        if config::is_provider_enabled(&config.nextcloud_credentials) {
            let provider = NextcloudProvider::new(creds.clone());
            check_provider("Nextcloud", &provider).await;
        }
    }

    #[cfg(feature = "box")]
    if let Some(creds) = &config.box_credentials {
        if config::is_provider_enabled(&config.box_credentials) {
            let provider = BoxProvider::new(creds.clone());
            check_provider("Box", &provider).await;
        }
    }

    #[cfg(feature = "mega")]
    if let Some(creds) = &config.mega_credentials {
        if config::is_provider_enabled(&config.mega_credentials) {
            let provider = MegaProvider::new(creds.clone());
            check_provider("MEGA", &provider).await;
        }
    }

    #[cfg(feature = "azure_blob")]
    if let Some(creds) = &config.azure_blob_credentials {
        if config::is_provider_enabled(&config.azure_blob_credentials) {
            let provider = AzureBlobProvider::new(creds.clone());
            check_provider("Azure Blob", &provider).await;
        }
    }

    #[cfg(feature = "gcs")]
    if let Some(creds) = &config.gcs_credentials {
        if config::is_provider_enabled(&config.gcs_credentials) {
            let provider = GCSProvider::new(creds.clone());
            check_provider("GCS", &provider).await;
        }
    }

    #[cfg(feature = "b2")]
    if let Some(creds) = &config.b2_credentials {
        if config::is_provider_enabled(&config.b2_credentials) {
            let provider = B2Provider::new(creds.clone());
            check_provider("B2", &provider).await;
        }
    }

    #[cfg(feature = "pcloud")]
    if let Some(creds) = &config.pcloud_credentials {
        if config::is_provider_enabled(&config.pcloud_credentials) {
            let provider = PCloudProvider::new(creds.clone());
            check_provider("pCloud", &provider).await;
        }
    }

    #[cfg(feature = "ipfs")]
    if let Some(creds) = &config.ipfs_credentials {
        if config::is_provider_enabled(&config.ipfs_credentials) {
            let provider = IPFSProvider::new(creds.clone());
            check_provider("IPFS", &provider).await;
        }
    }

    Ok(())
}
