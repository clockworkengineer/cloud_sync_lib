//! # cloud_sync_lib
//!
//! A library providing abstractions and client implementations for syncing files
//! with popular cloud storage backends, including Google Drive, Dropbox, and OneDrive.
//!
//! It supports both actual API clients (using OAuth2 refresh tokens) and a local
//! folder fallback simulation for offline development and testing.

pub mod providers;
pub mod traits;
pub mod rate_limit;
pub mod state;

pub use providers::{OAuthCredentials, WebDAVCredentials, S3Credentials, SFTPCredentials, NextcloudCredentials, MegaCredentials, AzureBlobCredentials, GCSCredentials, B2Credentials, PCloudCredentials, IPFSCredentials, SimulatedFallback, local_sim::LocalSimulation, CommonProviderSettings, ProviderConfig, EncryptedBackend, SyncMode};
pub use state::{SyncState, FileState};
#[cfg(feature = "google_drive")]
pub use providers::GoogleDriveProvider;
#[cfg(feature = "dropbox")]
pub use providers::DropboxProvider;
#[cfg(feature = "onedrive")]
pub use providers::OneDriveProvider;
#[cfg(feature = "webdav")]
pub use providers::WebDAVProvider;
#[cfg(feature = "s3")]
pub use providers::S3Provider;
#[cfg(feature = "sftp")]
pub use providers::SFTPProvider;
#[cfg(feature = "nextcloud")]
pub use providers::NextcloudProvider;
#[cfg(feature = "box")]
pub use providers::BoxProvider;
#[cfg(feature = "mega")]
pub use providers::MegaProvider;
#[cfg(feature = "azure_blob")]
pub use providers::AzureBlobProvider;
#[cfg(feature = "gcs")]
pub use providers::GCSProvider;
#[cfg(feature = "b2")]
pub use providers::B2Provider;
#[cfg(feature = "pcloud")]
pub use providers::PCloudProvider;
#[cfg(feature = "ipfs")]
pub use providers::IPFSProvider;
pub use traits::{StorageBackend, StorageError, StorageItem};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    /// Generic runner for SimulatedFallback flow test across different providers
    async fn run_simulated_flow_test<B>(provider_name: &str)
    where
        B: StorageBackend + 'static,
    {
        let temp_dir = tempdir().unwrap();
        let safe_name = provider_name.to_lowercase().replace(' ', "_");
        let provider_root = temp_dir.path().join(format!("{}_root", safe_name));
        let local_sim = LocalSimulation::new(provider_root.clone(), provider_name.to_string());
        let provider = SimulatedFallback::<B>::new(None, local_sim, provider_name, SyncMode::TwoWay);

        // Create a local temporary file to upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated cloud storage!").unwrap();

        // Upload
        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // Verify remote file exists
        let remote_file = provider_root.join("hello.txt");
        assert!(remote_file.exists());
        assert_eq!(std::fs::read_to_string(remote_file).unwrap().trim(), "Hello simulated cloud storage!");

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated cloud storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
        assert!(!provider_root.join("hello.txt").exists());
    }

    #[tokio::test]
    #[cfg(feature = "google_drive")]
    async fn test_google_drive_provider_flow() {
        run_simulated_flow_test::<GoogleDriveProvider>("Google Drive").await;
    }

    #[tokio::test]
    #[cfg(feature = "google_drive")]
    async fn test_google_drive_mock_http_flow() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // 1. Mock OAuth Token endpoint
        Mock::given(method("POST"))
            .and(path("/oauth"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "mocked-token-123"
            })))
            .mount(&server)
            .await;

        // 2. Mock File Search / Listing endpoint
        Mock::given(method("GET"))
            .and(path("/files"))
            .and(query_param("q", "name = 'hello.txt' and 'root' in parents and mimeType != 'application/vnd.google-apps.folder' and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [
                    {
                        "id": "hello-file-id",
                        "mimeType": "text/plain"
                    }
                ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/files"))
            .and(query_param("q", "'root' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [
                    {
                        "id": "hello-file-id",
                        "name": "hello.txt",
                        "size": "32",
                        "mimeType": "text/plain"
                    }
                ]
            })))
            .mount(&server)
            .await;

        // 3. Mock Upload endpoint (PATCH to /upload/hello-file-id)
        Mock::given(method("PATCH"))
            .and(path("/upload/hello-file-id"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        // 4. Mock Download endpoint (GET to /files/hello-file-id)
        Mock::given(method("GET"))
            .and(path("/files/hello-file-id"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Hello simulated cloud storage!"))
            .mount(&server)
            .await;

        // 5. Mock Delete endpoint (DELETE to /files/hello-file-id)
        Mock::given(method("DELETE"))
            .and(path("/files/hello-file-id"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("gdrive_root");
        
        let creds = OAuthCredentials {
            client_id: "mock_client".to_string(),
            client_secret: "mock_secret".to_string(),
            refresh_token: "mock_refresh".to_string(),
            common: CommonProviderSettings {
                destination_folder: None,
                enabled: None,
                sync_mode: None,
                encryption_password: None,
                ..Default::default()
            },
        };

        // Create provider and set endpoints to mock server
        let inner = GoogleDriveProvider::new(creds)
            .with_endpoints(
                format!("{}/oauth", server.uri()),
                format!("{}/files", server.uri()),
                format!("{}/upload", server.uri()),
            );
        let local_sim = LocalSimulation::new(provider_root.clone(), "Google Drive".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "Google Drive", SyncMode::TwoWay);

        // Upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated cloud storage!").unwrap();

        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated cloud storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    #[cfg(feature = "google_drive")]
    async fn test_google_drive_real_flow() {
        // Try to load private_config.toml first, then fall back to config.toml
        let mut config_path = std::path::Path::new("../private_config.toml");
        if !config_path.exists() {
            config_path = std::path::Path::new("../config.toml");
        }
        if !config_path.exists() {
            println!("Skipping real Google Drive test: configuration file not found.");
            return;
        }

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping real Google Drive test: failed to read config file");
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct TestConfig {
            google_credentials: Option<OAuthCredentials>,
        }

        let config: TestConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Skipping real Google Drive test: failed to parse config file ({:?})", e);
                return;
            }
        };

        let credentials = match config.google_credentials {
            Some(creds) => {
                if creds.client_secret.contains('*') 
                    || creds.client_secret.contains("PLACEHOLDER") 
                    || creds.client_id.contains("PLACEHOLDER")
                    || creds.client_id.is_empty() 
                {
                    println!("Skipping real Google Drive test: Credentials contain placeholder or masked secret.");
                    return;
                }
                creds
            }
            None => {
                println!("Skipping real Google Drive test: No google_credentials found in config file");
                return;
            }
        };

        println!("Running real Google Drive integration test...");
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("gdrive_root");
        let inner = GoogleDriveProvider::new(credentials);
        let local_sim = LocalSimulation::new(provider_root.clone(), "Google Drive".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "Google Drive", SyncMode::TwoWay);

        // Create a local temporary file to upload
        let file_name = format!("test_real_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        let local_file_path = temp_dir.path().join(&file_name);
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello real Google Drive!").unwrap();

        // Upload
        provider.upload(&local_file_path, &file_name).await.unwrap();

        // List files to find it
        let items = provider.list("").await.unwrap();
        let found = items.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(found, "Uploaded file was not found in the file listing");

        // Download
        let download_path = temp_dir.path().join("downloaded_real.txt");
        provider.download(&file_name, &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello real Google Drive!");

        // Delete
        provider.delete(&file_name).await.unwrap();

        // Verify it's deleted
        let items_after = provider.list("").await.unwrap();
        let found_after = items_after.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(!found_after, "File was not successfully deleted from Google Drive");
    }

    #[tokio::test]
    #[cfg(feature = "dropbox")]
    async fn test_dropbox_provider_simulated_flow() {
        run_simulated_flow_test::<DropboxProvider>("Dropbox").await;
    }

    #[tokio::test]
    #[cfg(feature = "dropbox")]
    async fn test_dropbox_mock_http_flow() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // 1. Mock OAuth Token endpoint
        Mock::given(method("POST"))
            .and(path("/oauth"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "mocked-dropbox-token-123"
            })))
            .mount(&server)
            .await;

        // 2. Mock Upload endpoint (POST to /content/upload)
        Mock::given(method("POST"))
            .and(path("/content/upload"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "hello.txt",
                "id": "id:12345"
            })))
            .mount(&server)
            .await;

        // 3. Mock Download endpoint (POST to /content/download)
        Mock::given(method("POST"))
            .and(path("/content/download"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Hello simulated cloud storage!"))
            .mount(&server)
            .await;

        // 4. Mock List endpoint (POST to /files/list_folder)
        Mock::given(method("POST"))
            .and(path("/files/list_folder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": [
                    {
                        ".tag": "file",
                        "name": "hello.txt",
                        "size": 32
                    }
                ],
                "cursor": "ZtkX...",
                "has_more": false
            })))
            .mount(&server)
            .await;

        // 5. Mock Delete endpoint (POST to /files/delete_v2)
        Mock::given(method("POST"))
            .and(path("/files/delete_v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "metadata": {
                    ".tag": "file",
                    "name": "hello.txt"
                }
            })))
            .mount(&server)
            .await;

        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("dropbox_root");
        
        let creds = OAuthCredentials {
            client_id: "mock_client".to_string(),
            client_secret: "mock_secret".to_string(),
            refresh_token: "mock_refresh".to_string(),
            common: CommonProviderSettings {
                destination_folder: None,
                enabled: None,
                sync_mode: None,
                encryption_password: None,
                ..Default::default()
            },
        };

        // Create provider and set endpoints to mock server
        let inner = DropboxProvider::new(creds)
            .with_endpoints(
                format!("{}/oauth", server.uri()),
                format!("{}/files", server.uri()),
                format!("{}/content", server.uri()),
            );
        let local_sim = LocalSimulation::new(provider_root.clone(), "Dropbox".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "Dropbox", SyncMode::TwoWay);

        // Upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated cloud storage!").unwrap();

        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated cloud storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    #[cfg(feature = "dropbox")]
    async fn test_dropbox_real_flow() {
        // Try to load private_config.toml first, then fall back to config.toml
        let mut config_path = std::path::Path::new("../private_config.toml");
        if !config_path.exists() {
            config_path = std::path::Path::new("../config.toml");
        }
        if !config_path.exists() {
            println!("Skipping real Dropbox test: configuration file not found.");
            return;
        }

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping real Dropbox test: failed to read config file");
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct TestConfig {
            dropbox_credentials: Option<OAuthCredentials>,
        }

        let config: TestConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Skipping real Dropbox test: failed to parse config file ({:?})", e);
                return;
            }
        };

        let credentials = match config.dropbox_credentials {
            Some(creds) => {
                if creds.client_secret.contains('*') 
                    || creds.client_secret.contains("PLACEHOLDER") 
                    || creds.client_id.contains("PLACEHOLDER")
                    || creds.client_id.is_empty() 
                {
                    println!("Skipping real Dropbox test: Credentials contain placeholder or masked secret.");
                    return;
                }
                creds
            }
            None => {
                println!("Skipping real Dropbox test: No dropbox_credentials found in config file");
                return;
            }
        };

        println!("Running real Dropbox integration test...");
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("dropbox_root");
        let inner = DropboxProvider::new(credentials);
        let local_sim = LocalSimulation::new(provider_root.clone(), "Dropbox".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "Dropbox", SyncMode::TwoWay);

        // Create a local temporary file to upload
        let file_name = format!("test_real_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        let local_file_path = temp_dir.path().join(&file_name);
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello real Dropbox!").unwrap();

        // Upload
        provider.upload(&local_file_path, &file_name).await.unwrap();

        // List files to find it
        let items = provider.list("").await.unwrap();
        let found = items.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(found, "Uploaded file was not found in the file listing");

        // Download
        let download_path = temp_dir.path().join("downloaded_real.txt");
        provider.download(&file_name, &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello real Dropbox!");

        // Delete
        provider.delete(&file_name).await.unwrap();

        // Verify it's deleted
        let items_after = provider.list("").await.unwrap();
        let found_after = items_after.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(!found_after, "File was not successfully deleted from Dropbox");
    }

    #[tokio::test]
    #[cfg(feature = "onedrive")]
    async fn test_onedrive_provider_simulated_flow() {
        run_simulated_flow_test::<OneDriveProvider>("OneDrive").await;
    }

    #[tokio::test]
    #[cfg(feature = "onedrive")]
    async fn test_onedrive_mock_http_flow() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // 1. Mock OAuth Token endpoint
        Mock::given(method("POST"))
            .and(path("/oauth"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "mocked-onedrive-token-123"
            })))
            .mount(&server)
            .await;

        // 2. Mock Upload endpoint (PUT to /me/drive/root:/hello.txt:/content)
        Mock::given(method("PUT"))
            .and(path("/me/drive/root:/hello.txt:/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "hello.txt"
            })))
            .mount(&server)
            .await;

        // 3. Mock Download endpoint (GET to /me/drive/root:/hello.txt:/content)
        Mock::given(method("GET"))
            .and(path("/me/drive/root:/hello.txt:/content"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Hello simulated cloud storage!"))
            .mount(&server)
            .await;

        // 4. Mock List endpoint (GET to /me/drive/root/children)
        Mock::given(method("GET"))
            .and(path("/me/drive/root/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "name": "hello.txt",
                        "size": 32
                    }
                ]
            })))
            .mount(&server)
            .await;

        // 5. Mock Delete endpoint (DELETE to /me/drive/root:/hello.txt)
        Mock::given(method("DELETE"))
            .and(path("/me/drive/root:/hello.txt"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("onedrive_root");
        
        let creds = OAuthCredentials {
            client_id: "mock_client".to_string(),
            client_secret: "mock_secret".to_string(),
            refresh_token: "mock_refresh".to_string(),
            common: CommonProviderSettings {
                destination_folder: None,
                enabled: None,
                sync_mode: None,
                encryption_password: None,
                ..Default::default()
            },
        };

        // Create provider and set endpoints to mock server
        let inner = OneDriveProvider::new(creds)
            .with_endpoints(
                format!("{}/oauth", server.uri()),
                server.uri(),
            );
        let local_sim = LocalSimulation::new(provider_root.clone(), "OneDrive".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "OneDrive", SyncMode::TwoWay);

        // Upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated cloud storage!").unwrap();

        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated cloud storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    #[cfg(feature = "onedrive")]
    async fn test_onedrive_real_flow() {
        // Try to load private_config.toml first, then fall back to config.toml
        let mut config_path = std::path::Path::new("../private_config.toml");
        if !config_path.exists() {
            config_path = std::path::Path::new("../config.toml");
        }
        if !config_path.exists() {
            println!("Skipping real OneDrive test: configuration file not found.");
            return;
        }

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping real OneDrive test: failed to read config file");
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct TestConfig {
            onedrive_credentials: Option<OAuthCredentials>,
        }

        let config: TestConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Skipping real OneDrive test: failed to parse config file ({:?})", e);
                return;
            }
        };

        let credentials = match config.onedrive_credentials {
            Some(creds) => {
                if creds.client_secret.contains('*') 
                    || creds.client_secret.contains("PLACEHOLDER") 
                    || creds.client_id.contains("PLACEHOLDER")
                    || creds.client_id.is_empty() 
                {
                    println!("Skipping real OneDrive test: Credentials contain placeholder or masked secret.");
                    return;
                }
                creds
            }
            None => {
                println!("Skipping real OneDrive test: No onedrive_credentials found in config file");
                return;
            }
        };

        println!("Running real OneDrive integration test...");
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("onedrive_root");
        let inner = OneDriveProvider::new(credentials);
        let local_sim = LocalSimulation::new(provider_root.clone(), "OneDrive".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "OneDrive", SyncMode::TwoWay);

        // Create a local temporary file to upload
        let file_name = format!("test_real_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        let local_file_path = temp_dir.path().join(&file_name);
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello real OneDrive!").unwrap();

        // Upload
        provider.upload(&local_file_path, &file_name).await.unwrap();

        // List files to find it
        let items = provider.list("").await.unwrap();
        let found = items.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(found, "Uploaded file was not found in the file listing");

        // Download
        let download_path = temp_dir.path().join("downloaded_real.txt");
        provider.download(&file_name, &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello real OneDrive!");

        // Delete
        provider.delete(&file_name).await.unwrap();

        // Verify it's deleted
        let items_after = provider.list("").await.unwrap();
        let found_after = items_after.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(!found_after, "File was not successfully deleted from OneDrive");
    }

    #[tokio::test]
    #[cfg(feature = "webdav")]
    async fn test_webdav_provider_simulated_flow() {
        run_simulated_flow_test::<WebDAVProvider>("WebDAV").await;
    }

    #[tokio::test]
    #[cfg(feature = "webdav")]
    async fn test_webdav_mock_http_flow() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // 1. Mock MKCOL directory creation (matches /MySyncFolder)
        Mock::given(method("MKCOL"))
            .and(path("/MySyncFolder"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        // 2. Mock Upload endpoint (PUT to /MySyncFolder/hello.txt)
        Mock::given(method("PUT"))
            .and(path("/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        // 3. Mock Download endpoint (GET to /MySyncFolder/hello.txt)
        Mock::given(method("GET"))
            .and(path("/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Hello simulated WebDAV storage!"))
            .mount(&server)
            .await;

        // 4. Mock List endpoint (PROPFIND to /MySyncFolder)
        let propfind_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/MySyncFolder</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/MySyncFolder/hello.txt</d:href>
    <d:propstat>
      <d:prop>
        <d:getcontentlength>32</d:getcontentlength>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        Mock::given(method("PROPFIND"))
            .and(path("/MySyncFolder/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(propfind_xml))
            .mount(&server)
            .await;

        // 5. Mock Delete endpoint (DELETE to /MySyncFolder/hello.txt)
        Mock::given(method("DELETE"))
            .and(path("/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("webdav_root");
        
        let creds = WebDAVCredentials {
            url: server.uri(),
            username: "username".to_string(),
            password: "password".to_string(),
            common: CommonProviderSettings {
                destination_folder: Some("MySyncFolder".to_string()),
                enabled: None,
                sync_mode: None,
                encryption_password: None,
                ..Default::default()
            },
        };

        // Create provider and set endpoints to mock server
        let inner = WebDAVProvider::new(creds).with_endpoints(server.uri());
        let local_sim = LocalSimulation::new(provider_root.clone(), "WebDAV".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "WebDAV", SyncMode::TwoWay);

        // Upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated WebDAV storage!").unwrap();

        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated WebDAV storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    #[cfg(feature = "webdav")]
    async fn test_webdav_real_flow() {
        let mut config_path = std::path::Path::new("../private_config.toml");
        if !config_path.exists() {
            config_path = std::path::Path::new("../config.toml");
        }
        if !config_path.exists() {
            println!("Skipping real WebDAV test: configuration file not found.");
            return;
        }

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping real WebDAV test: failed to read config file");
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct TestConfig {
            webdav_credentials: Option<WebDAVCredentials>,
        }

        let config: TestConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Skipping real WebDAV test: failed to parse config file ({:?})", e);
                return;
            }
        };

        let credentials = match config.webdav_credentials {
            Some(creds) => {
                if creds.username.contains("PLACEHOLDER") 
                    || creds.username.is_empty() 
                {
                    println!("Skipping real WebDAV test: Credentials contain placeholder or empty username.");
                    return;
                }
                creds
            }
            None => {
                println!("Skipping real WebDAV test: No webdav_credentials found in config file");
                return;
            }
        };

        println!("Running real WebDAV integration test...");
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("webdav_root");
        let inner = WebDAVProvider::new(credentials);
        let local_sim = LocalSimulation::new(provider_root.clone(), "WebDAV".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "WebDAV", SyncMode::TwoWay);

        // Create a local temporary file to upload
        let file_name = format!("test_real_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        let local_file_path = temp_dir.path().join(&file_name);
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello real WebDAV!").unwrap();

        // Upload
        provider.upload(&local_file_path, &file_name).await.unwrap();

        // List files to find it
        let items = provider.list("").await.unwrap();
        let found = items.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(found, "Uploaded file was not found in the file listing");

        // Download
        let download_path = temp_dir.path().join("downloaded_real.txt");
        provider.download(&file_name, &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello real WebDAV!");

        // Delete
        provider.delete(&file_name).await.unwrap();

        // Verify it's deleted
        let items_after = provider.list("").await.unwrap();
        let found_after = items_after.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(!found_after, "File was not successfully deleted from WebDAV");
    }

    #[tokio::test]
    #[cfg(feature = "s3")]
    async fn test_s3_provider_simulated_flow() {
        run_simulated_flow_test::<S3Provider>("S3").await;
    }

    #[tokio::test]
    #[cfg(feature = "s3")]
    async fn test_s3_mock_http_flow() {
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // 1. Mock Upload endpoint (PUT to /test-bucket/MySyncFolder/hello.txt)
        Mock::given(method("PUT"))
            .and(path("/test-bucket/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        // 2. Mock Download endpoint (GET to /test-bucket/MySyncFolder/hello.txt)
        Mock::given(method("GET"))
            .and(path("/test-bucket/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Hello simulated S3 storage!"))
            .mount(&server)
            .await;

        // 3. Mock List endpoint (GET to /test-bucket or /test-bucket/)
        let list_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
    <Name>test-bucket</Name>
    <Prefix>MySyncFolder/</Prefix>
    <KeyCount>1</KeyCount>
    <MaxKeys>1000</MaxKeys>
    <IsTruncated>false</IsTruncated>
    <Contents>
        <Key>MySyncFolder/hello.txt</Key>
        <LastModified>2026-06-26T12:00:00.000Z</LastModified>
        <ETag>&quot;3a3f&quot;</ETag>
        <Size>32</Size>
        <StorageClass>STANDARD</StorageClass>
    </Contents>
</ListBucketResult>"#;

        Mock::given(method("GET"))
            .and(path_regex(r"^/test-bucket/?$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_xml))
            .mount(&server)
            .await;

        // 4. Mock Delete endpoint (DELETE to /test-bucket/MySyncFolder/hello.txt)
        Mock::given(method("DELETE"))
            .and(path("/test-bucket/MySyncFolder/hello.txt"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("s3_root");
        
        let creds = S3Credentials {
            bucket: "test-bucket".to_string(),
            region: "us-east-1".to_string(),
            access_key_id: "access_key".to_string(),
            secret_access_key: "secret_key".to_string(),
            endpoint: Some(server.uri()),
            common: CommonProviderSettings {
                destination_folder: Some("MySyncFolder".to_string()),
                enabled: None,
                sync_mode: None,
                encryption_password: None,
                ..Default::default()
            },
        };

        // Create provider and set endpoints to mock server
        let inner = S3Provider::new(creds).with_endpoints(server.uri());
        let local_sim = LocalSimulation::new(provider_root.clone(), "S3".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "S3", SyncMode::TwoWay);

        // Upload
        let local_file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello simulated S3 storage!").unwrap();

        provider.upload(&local_file_path, "hello.txt").await.unwrap();

        // List
        let items = provider.list("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path.to_string_lossy(), "hello.txt");

        // Download
        let download_path = temp_dir.path().join("downloaded.txt");
        provider.download("hello.txt", &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello simulated S3 storage!");

        // Delete
        provider.delete("hello.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    #[cfg(feature = "s3")]
    async fn test_s3_real_flow() {
        let mut config_path = std::path::Path::new("../private_config.toml");
        if !config_path.exists() {
            config_path = std::path::Path::new("../config.toml");
        }
        if !config_path.exists() {
            println!("Skipping real S3 test: configuration file not found.");
            return;
        }

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping real S3 test: failed to read config file");
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct TestConfig {
            s3_credentials: Option<S3Credentials>,
        }

        let config: TestConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Skipping real S3 test: failed to parse config file ({:?})", e);
                return;
            }
        };

        let credentials = match config.s3_credentials {
            Some(creds) => {
                if creds.access_key_id.contains("PLACEHOLDER") 
                    || creds.access_key_id.is_empty() 
                {
                    println!("Skipping real S3 test: Credentials contain placeholder or empty key.");
                    return;
                }
                creds
            }
            None => {
                println!("Skipping real S3 test: No s3_credentials found in config file");
                return;
            }
        };

        println!("Running real S3 integration test...");
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("s3_root");
        let inner = S3Provider::new(credentials);
        let local_sim = LocalSimulation::new(provider_root.clone(), "S3".to_string());
        let provider = SimulatedFallback::new(Some(inner), local_sim, "S3", SyncMode::TwoWay);

        // Create a local temporary file to upload
        let file_name = format!("test_real_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        let local_file_path = temp_dir.path().join(&file_name);
        let mut file = File::create(&local_file_path).unwrap();
        writeln!(file, "Hello real S3!").unwrap();

        // Upload
        provider.upload(&local_file_path, &file_name).await.unwrap();

        // List files to find it
        let items = provider.list("").await.unwrap();
        let found = items.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(found, "Uploaded file was not found in the file listing");

        // Download
        let download_path = temp_dir.path().join("downloaded_real.txt");
        provider.download(&file_name, &download_path).await.unwrap();
        assert!(download_path.exists());
        assert_eq!(std::fs::read_to_string(download_path).unwrap().trim(), "Hello real S3!");

        // Delete
        provider.delete(&file_name).await.unwrap();

        // Verify it's deleted
        let items_after = provider.list("").await.unwrap();
        let found_after = items_after.iter().any(|item| item.path.to_string_lossy() == file_name);
        assert!(!found_after, "File was not successfully deleted from S3");
    }

    #[tokio::test]
    #[cfg(feature = "sftp")]
    async fn test_sftp_provider_simulated_flow() {
        run_simulated_flow_test::<SFTPProvider>("SFTP").await;
    }

    #[tokio::test]
    #[cfg(feature = "nextcloud")]
    async fn test_nextcloud_provider_simulated_flow() {
        run_simulated_flow_test::<NextcloudProvider>("Nextcloud").await;
    }

    #[tokio::test]
    #[cfg(feature = "box")]
    async fn test_box_provider_simulated_flow() {
        run_simulated_flow_test::<BoxProvider>("Box").await;
    }

    #[tokio::test]
    #[cfg(feature = "mega")]
    async fn test_mega_provider_simulated_flow() {
        run_simulated_flow_test::<MegaProvider>("MEGA").await;
    }

    #[tokio::test]
    #[cfg(feature = "azure_blob")]
    async fn test_azure_blob_provider_simulated_flow() {
        run_simulated_flow_test::<AzureBlobProvider>("Azure Blob").await;
    }

    #[tokio::test]
    #[cfg(feature = "gcs")]
    async fn test_gcs_provider_simulated_flow() {
        run_simulated_flow_test::<GCSProvider>("GCS").await;
    }

    #[tokio::test]
    #[cfg(feature = "b2")]
    async fn test_b2_provider_simulated_flow() {
        run_simulated_flow_test::<B2Provider>("B2").await;
    }

    #[tokio::test]
    #[cfg(feature = "pcloud")]
    async fn test_pcloud_provider_simulated_flow() {
        run_simulated_flow_test::<PCloudProvider>("pCloud").await;
    }

    #[tokio::test]
    #[cfg(feature = "ipfs")]
    async fn test_ipfs_provider_simulated_flow() {
        run_simulated_flow_test::<IPFSProvider>("IPFS").await;
    }
}
