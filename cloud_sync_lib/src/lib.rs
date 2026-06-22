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
}
