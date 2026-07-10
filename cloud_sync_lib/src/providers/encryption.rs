//! Client-side zero-knowledge encryption backend provider implementation.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use async_trait::async_trait;
use rand::RngCore;
use sha2::{Sha256, Digest};
use std::path::Path;
use tokio::fs;

/// Wrapper that implements `StorageBackend` and automatically encrypts/decrypts file content.
pub struct EncryptedBackend<B: StorageBackend> {
    inner: B,
    key: Key<Aes256Gcm>,
    name: String,
}

impl<B: StorageBackend> EncryptedBackend<B> {
    /// Creates a new `EncryptedBackend` around the inner backend using a password.
    pub fn new(inner: B, password: &str) -> Self {
        // Derive a 256-bit key from password using SHA-256
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let hash = hasher.finalize();
        let key = *Key::<Aes256Gcm>::from_slice(&hash);

        let name = format!("Encrypted({})", inner.name());
        Self {
            inner,
            key,
            name,
        }
    }
}

#[async_trait]
impl<B: StorageBackend> StorageBackend for EncryptedBackend<B> {
    fn name(&self) -> &str {
        &self.name
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        use zeroize::Zeroize;
        // 1. Read plaintext local file
        let mut plaintext = fs::read(local_path).await?;

        // 2. Generate random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // 3. Encrypt
        let cipher = Aes256Gcm::new(&self.key);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_slice())
            .map_err(|e| StorageError::Provider { message: format!("Encryption failed: {}", e), status: None })?;

        // Clean up plaintext memory
        plaintext.zeroize();

        // 4. Prepend nonce to ciphertext
        let mut payload = Vec::with_capacity(12 + ciphertext.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);

        // Clean up nonce bytes in memory
        nonce_bytes.zeroize();

        // 5. Write to a temporary file
        let temp_dir = std::env::temp_dir();
        // Generate a random temporary filename to prevent collisions
        let temp_filename = format!("enc_sync_tmp_{}.enc", rand::random::<u64>());
        let temp_path = temp_dir.join(temp_filename);
        fs::write(&temp_path, payload).await?;

        // 6. Upload temporary file
        let upload_res = self.inner.upload(&temp_path, remote_path).await;

        // 7. Cleanup temp file
        let _ = fs::remove_file(&temp_path).await;

        upload_res
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        use zeroize::Zeroize;
        // 1. Create a temporary path for downloaded encrypted file
        let temp_dir = std::env::temp_dir();
        let temp_filename = format!("dec_sync_tmp_{}.enc", rand::random::<u64>());
        let temp_path = temp_dir.join(temp_filename);

        // 2. Download from remote to temp path
        self.inner.download(remote_path, &temp_path).await?;

        // 3. Read encrypted payload
        let mut payload = fs::read(&temp_path).await?;
        let _ = fs::remove_file(&temp_path).await;

        if payload.len() < 12 {
            payload.zeroize();
            return Err(StorageError::Provider { message: "Invalid encrypted file: too short".to_string(), status: None });
        }

        // 4. Split nonce and ciphertext
        let (nonce_bytes, ciphertext) = payload.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // 5. Decrypt
        let cipher = Aes256Gcm::new(&self.key);
        let mut plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| StorageError::Provider { message: format!("Decryption failed: {}", e), status: None })?;

        // Clean up payload memory
        payload.zeroize();

        // 6. Write decrypted content to target path
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(local_path, &plaintext).await?;

        // Clean up decrypted plaintext memory
        plaintext.zeroize();

        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.delete(remote_path).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let items = self.inner.list(remote_path).await?;
        let mut mapped = Vec::with_capacity(items.len());
        for mut item in items {
            // Subtract nonce (12 bytes) + tag (16 bytes) overhead from file size
            if !item.is_dir {
                item.size = item.size.saturating_sub(28);
            }
            mapped.push(item);
        }
        Ok(mapped)
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }
}

impl<B: StorageBackend> Drop for EncryptedBackend<B> {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.as_mut_slice().zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_encrypted_backend_flow() {
        let temp_dir = tempdir().unwrap();
        let local_root = temp_dir.path().join("local");
        let remote_root = temp_dir.path().join("remote");

        fs::create_dir_all(&local_root).await.unwrap();
        fs::create_dir_all(&remote_root).await.unwrap();

        let local_sim = LocalSimulation::new(remote_root.clone(), "MockRemote".to_string());
        let enc_backend = EncryptedBackend::new(local_sim, "supersecretpassword");

        // 1. Upload a file
        let local_file = local_root.join("test.txt");
        let content = b"Hello, Zero-Knowledge Sync World!";
        fs::write(&local_file, content).await.unwrap();

        enc_backend.upload(&local_file, "remote_test.txt").await.unwrap();

        // Verify the file stored on the remote is encrypted and different
        let remote_stored_file = remote_root.join("remote_test.txt");
        let encrypted_content = fs::read(&remote_stored_file).await.unwrap();
        assert_ne!(encrypted_content, content);
        assert!(encrypted_content.len() > content.len());

        // Verify list returns decrypted size
        let list = enc_backend.list("").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].size, content.len() as u64);

        // 2. Download and decrypt
        let downloaded_file = local_root.join("downloaded.txt");
        enc_backend.download("remote_test.txt", &downloaded_file).await.unwrap();

        let decrypted_content = fs::read(&downloaded_file).await.unwrap();
        assert_eq!(decrypted_content, content);
    }
}
