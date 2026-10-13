//! Client-side zero-knowledge encryption backend provider implementation.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use async_trait::async_trait;
use rand::RngCore;
use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use zeroize::Zeroize;

const MAGIC: &[u8; 4] = b"CSYC";
const VERSION: u8 = 1;
const CHUNK_SIZE: usize = 64 * 1024; // 64 KiB chunks

/// Wrapper that implements `StorageBackend` and automatically encrypts/decrypts file content
/// using authenticated 64KB streaming AEAD (AES-256-GCM) with hardened per-file salts.
pub struct EncryptedBackend<B: StorageBackend> {
    inner: B,
    password: String,
    legacy_key: Key<Aes256Gcm>,
    name: String,
}

impl<B: StorageBackend> EncryptedBackend<B> {
    /// Creates a new `EncryptedBackend` around the inner backend using a password.
    pub fn new(inner: B, password: &str) -> Self {
        // Legacy key derivation for backward-compatible decryption of existing files
        let legacy_salt = b"cloud_sync_lib_salt_aes256gcm";
        let mut legacy_key_bytes = [0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(10_000).unwrap(),
            legacy_salt,
            password.as_bytes(),
            &mut legacy_key_bytes,
        );
        let legacy_key = *Key::<Aes256Gcm>::from_slice(&legacy_key_bytes);
        legacy_key_bytes.zeroize();

        let name = format!("Encrypted({})", inner.name());
        Self {
            inner,
            password: password.to_string(),
            legacy_key,
            name,
        }
    }

    /// Derives a hardened per-file 256-bit AES key using PBKDF2-HMAC-SHA256 (100,000 iterations).
    fn derive_file_key(&self, salt: &[u8]) -> Key<Aes256Gcm> {
        let mut key_bytes = [0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(100_000).unwrap(),
            salt,
            self.password.as_bytes(),
            &mut key_bytes,
        );
        let key = *Key::<Aes256Gcm>::from_slice(&key_bytes);
        key_bytes.zeroize();
        key
    }
}

#[async_trait]
impl<B: StorageBackend> StorageBackend for EncryptedBackend<B> {
    fn name(&self) -> &str {
        &self.name
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let mut file = fs::File::open(local_path).await?;
        let file_size = file.metadata().await?.len();

        // 1. Generate unique random 16-byte salt and 8-byte base nonce for this file
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let mut base_nonce = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut base_nonce);

        let file_key = self.derive_file_key(&salt);
        let cipher = Aes256Gcm::new(&file_key);

        // 2. Open temporary ciphertext staging file with restricted permissions (0600 on Unix)
        let temp_dir = std::env::temp_dir();
        let temp_filename = format!("enc_sync_tmp_{}.enc", rand::random::<u64>());
        let temp_path = temp_dir.join(temp_filename);

        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut temp_file = options.open(&temp_path).await?;

        // 3. Write Vault Header: MAGIC (4B) + VERSION (1B) + SALT (16B) + BASE_NONCE (8B)
        temp_file.write_all(MAGIC).await?;
        temp_file.write_u8(VERSION).await?;
        temp_file.write_all(&salt).await?;
        temp_file.write_all(&base_nonce).await?;

        // 4. Stream and encrypt in 64 KiB chunks
        let mut chunk_index: u32 = 0;
        let mut total_read: u64 = 0;
        let mut buf = vec![0u8; CHUNK_SIZE];

        loop {
            let bytes_read = file.read(&mut buf).await?;
            total_read += bytes_read as u64;
            let is_last: u8 = if total_read >= file_size { 1 } else { 0 };

            // Construct 12-byte AEAD nonce: base_nonce (8B) + chunk_index (4B)
            let mut nonce_bytes = [0u8; 12];
            nonce_bytes[..8].copy_from_slice(&base_nonce);
            nonce_bytes[8..12].copy_from_slice(&chunk_index.to_be_bytes());
            let nonce = Nonce::from_slice(&nonce_bytes);

            // Authenticated Associated Data (AAD): chunk_index (4B) + is_last (1B)
            let mut aad = [0u8; 5];
            aad[..4].copy_from_slice(&chunk_index.to_be_bytes());
            aad[4] = is_last;

            let payload = Payload {
                msg: &buf[..bytes_read],
                aad: &aad,
            };

            let ciphertext =
                cipher
                    .encrypt(nonce, payload)
                    .map_err(|e| StorageError::Provider {
                        message: format!("Encryption failed: {}", e),
                        status: None,
                    })?;

            temp_file.write_u32(ciphertext.len() as u32).await?;
            temp_file.write_u8(is_last).await?;
            temp_file.write_all(&ciphertext).await?;

            chunk_index = chunk_index.saturating_add(1);
            if is_last == 1 {
                break;
            }
        }
        temp_file.flush().await?;
        buf.zeroize();

        // 5. Upload encrypted file to remote destination
        let upload_res = self.inner.upload(&temp_path, remote_path).await;

        // 6. Cleanup temp file
        let _ = fs::remove_file(&temp_path).await;

        upload_res
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        // 1. Create a temporary path for downloaded encrypted file
        let temp_dir = std::env::temp_dir();
        let temp_filename = format!("dec_sync_tmp_{}.enc", rand::random::<u64>());
        let temp_path = temp_dir.join(temp_filename);

        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        {
            let _file = options.open(&temp_path).await?;
        }

        // 2. Download from remote to temp path
        self.inner.download(remote_path, &temp_path).await?;

        let mut enc_file = fs::File::open(&temp_path).await?;
        let enc_size = enc_file.metadata().await?.len();

        // Check if file uses modern chunked streaming format
        if enc_size >= 29 {
            let mut magic = [0u8; 4];
            enc_file.read_exact(&mut magic).await?;

            if &magic == MAGIC {
                let version = enc_file.read_u8().await?;
                if version != VERSION {
                    let _ = fs::remove_file(&temp_path).await;
                    return Err(StorageError::Provider {
                        message: format!("Unsupported encrypted version: {}", version),
                        status: None,
                    });
                }

                let mut salt = [0u8; 16];
                enc_file.read_exact(&mut salt).await?;
                let mut base_nonce = [0u8; 8];
                enc_file.read_exact(&mut base_nonce).await?;

                let file_key = self.derive_file_key(&salt);
                let cipher = Aes256Gcm::new(&file_key);

                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                let mut dest_file = fs::File::create(local_path).await?;

                let mut chunk_index: u32 = 0;
                loop {
                    let chunk_len = match enc_file.read_u32().await {
                        Ok(len) => len as usize,
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                        Err(e) => {
                            let _ = fs::remove_file(&temp_path).await;
                            return Err(StorageError::Io(e));
                        }
                    };

                    let is_last = enc_file.read_u8().await?;
                    let mut cipher_buf = vec![0u8; chunk_len];
                    enc_file.read_exact(&mut cipher_buf).await?;

                    let mut nonce_bytes = [0u8; 12];
                    nonce_bytes[..8].copy_from_slice(&base_nonce);
                    nonce_bytes[8..12].copy_from_slice(&chunk_index.to_be_bytes());
                    let nonce = Nonce::from_slice(&nonce_bytes);

                    let mut aad = [0u8; 5];
                    aad[..4].copy_from_slice(&chunk_index.to_be_bytes());
                    aad[4] = is_last;

                    let payload = Payload {
                        msg: &cipher_buf,
                        aad: &aad,
                    };

                    let mut plaintext =
                        cipher
                            .decrypt(nonce, payload)
                            .map_err(|e| StorageError::Provider {
                                message: format!("Decryption failed: {}", e),
                                status: None,
                            })?;

                    dest_file.write_all(&plaintext).await?;
                    plaintext.zeroize();

                    chunk_index = chunk_index.saturating_add(1);
                    if is_last == 1 {
                        break;
                    }
                }
                dest_file.flush().await?;
                let _ = fs::remove_file(&temp_path).await;
                return Ok(());
            } else {
                enc_file.seek(std::io::SeekFrom::Start(0)).await?;
            }
        }

        // 3. Fallback to legacy v0 single-pass decryption
        let mut payload = fs::read(&temp_path).await?;
        let _ = fs::remove_file(&temp_path).await;

        if payload.len() < 12 {
            payload.zeroize();
            return Err(StorageError::Provider {
                message: "Invalid encrypted file: too short".to_string(),
                status: None,
            });
        }

        let (nonce_bytes, ciphertext) = payload.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = Aes256Gcm::new(&self.legacy_key);
        let mut plaintext =
            cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| StorageError::Provider {
                    message: format!("Decryption failed: {}", e),
                    status: None,
                })?;

        payload.zeroize();

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(local_path, &plaintext).await?;
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
            if !item.is_dir {
                // Deduct encryption framing overhead
                if item.size >= 29 {
                    let body_size = item.size - 29;
                    let num_chunks = body_size.div_ceil(65557);
                    let overhead = 29 + num_chunks * 21;
                    item.size = item.size.saturating_sub(overhead);
                } else {
                    item.size = item.size.saturating_sub(28);
                }
            }
            mapped.push(item);
        }
        Ok(mapped)
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        self.inner.get_latest_cursor().await
    }

    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<cloud_sync_core::RemoteChange>, String)>, StorageError> {
        self.inner.fetch_changes(cursor).await
    }

    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }

    fn simulation_reason(&self) -> Option<String> {
        self.inner.simulation_reason()
    }
}

impl<B: StorageBackend> Drop for EncryptedBackend<B> {
    fn drop(&mut self) {
        self.password.zeroize();
        self.legacy_key.as_mut_slice().zeroize();
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
        assert_eq!(enc_backend.name(), "Encrypted(MockRemote)");

        // 1. Upload a file
        let local_file = local_root.join("test.txt");
        let content = b"Hello, Zero-Knowledge Sync World!";
        fs::write(&local_file, content).await.unwrap();

        enc_backend
            .upload(&local_file, "remote_test.txt")
            .await
            .unwrap();

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
        enc_backend
            .download("remote_test.txt", &downloaded_file)
            .await
            .unwrap();

        let decrypted_content = fs::read(&downloaded_file).await.unwrap();
        assert_eq!(decrypted_content, content);
    }

    #[tokio::test]
    async fn test_encrypted_backend_edge_cases() {
        let temp_dir = tempdir().unwrap();
        let remote_root = temp_dir.path().join("remote");
        fs::create_dir_all(&remote_root).await.unwrap();

        let local_sim = LocalSimulation::new(remote_root.clone(), "MockRemote".to_string());
        let enc_backend = EncryptedBackend::new(local_sim, "supersecretpassword");

        // Test delete and create_folder delegation
        enc_backend.create_folder("new_dir").await.unwrap();
        assert!(remote_root.join("new_dir").exists());

        // Create a file that is too short
        let short_file = remote_root.join("short.txt");
        fs::write(&short_file, b"too_short").await.unwrap();

        let local_dest = temp_dir.path().join("dest.txt");
        let res = enc_backend.download("short.txt", &local_dest).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("too short"));

        // Test delete delegation
        enc_backend.delete("short.txt").await.unwrap();
        assert!(!short_file.exists());
    }

    #[tokio::test]
    async fn test_encrypted_backend_multi_chunk_stream() {
        let temp_dir = tempdir().unwrap();
        let local_root = temp_dir.path().join("local");
        let remote_root = temp_dir.path().join("remote");

        fs::create_dir_all(&local_root).await.unwrap();
        fs::create_dir_all(&remote_root).await.unwrap();

        let local_sim = LocalSimulation::new(remote_root.clone(), "MockRemote".to_string());
        let enc_backend = EncryptedBackend::new(local_sim, "hardenedpassword123");

        // 150 KiB file spans three 64 KiB chunks (64KB, 64KB, 22KB)
        let large_file = local_root.join("multi_chunk.bin");
        let mut large_data = vec![0u8; 150 * 1024];
        for (i, b) in large_data.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        fs::write(&large_file, &large_data).await.unwrap();

        enc_backend
            .upload(&large_file, "remote_multi.bin")
            .await
            .unwrap();

        // Download and verify integrity
        let downloaded_file = local_root.join("downloaded_multi.bin");
        enc_backend
            .download("remote_multi.bin", &downloaded_file)
            .await
            .unwrap();

        let decrypted = fs::read(&downloaded_file).await.unwrap();
        assert_eq!(decrypted, large_data);
    }

    #[tokio::test]
    async fn test_encrypted_backend_tamper_detection() {
        let temp_dir = tempdir().unwrap();
        let local_root = temp_dir.path().join("local");
        let remote_root = temp_dir.path().join("remote");

        fs::create_dir_all(&local_root).await.unwrap();
        fs::create_dir_all(&remote_root).await.unwrap();

        let local_sim = LocalSimulation::new(remote_root.clone(), "MockRemote".to_string());
        let enc_backend = EncryptedBackend::new(local_sim, "password123");

        let file = local_root.join("tamper_test.txt");
        fs::write(&file, b"Authenticate me accurately!")
            .await
            .unwrap();

        enc_backend
            .upload(&file, "tamper_remote.txt")
            .await
            .unwrap();

        // Tamper with one byte in the ciphertext on remote
        let remote_file = remote_root.join("tamper_remote.txt");
        let mut corrupted = fs::read(&remote_file).await.unwrap();
        let corrupt_idx = corrupted.len() - 5;
        corrupted[corrupt_idx] ^= 0xFF; // Flip bits
        fs::write(&remote_file, corrupted).await.unwrap();

        // Decryption should fail due to AEAD tag mismatch
        let dest = local_root.join("tamper_dest.txt");
        let res = enc_backend.download("tamper_remote.txt", &dest).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Decryption failed"));
    }
}
