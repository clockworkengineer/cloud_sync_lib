pub mod providers;
pub mod traits;

pub use providers::{DropboxProvider, GoogleDriveProvider, OneDriveProvider, OAuthCredentials};
pub use traits::{StorageBackend, StorageError, StorageItem};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_google_drive_provider_flow() {
        let temp_dir = tempdir().unwrap();
        let provider_root = temp_dir.path().join("gdrive_root");
        let provider = GoogleDriveProvider::new(&provider_root, None).await.unwrap();

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
            destination_folder: None,
        };

        // Create provider and set endpoints to mock server
        let provider = GoogleDriveProvider::new(&provider_root, Some(creds))
            .await
            .unwrap()
            .with_endpoints(
                format!("{}/oauth", server.uri()),
                format!("{}/files", server.uri()),
                format!("{}/upload", server.uri()),
            );

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
        let provider = GoogleDriveProvider::new(&provider_root, Some(credentials)).await.unwrap();

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
}
