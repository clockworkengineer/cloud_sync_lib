//! Transparent Lossless Compression Middleware Layer (Rclone `compress` / Kopia equivalent).
//!
//! Compresses file payloads before uploading to any [`StorageBackend`] and transparently
//! decompresses them upon download. Uses a self-identifying framing header (`CLZ1`) with
//! pure-Rust LZ77 + Run-Length Encoding so that:
//! 1. Zero external C libraries are needed (preserving 100% pure-Rust Android builds).
//! 2. Incompressible files are stored raw without size inflation.
//! 3. Pre-existing uncompressed files on the remote backend are downloaded transparently.

use crate::layers::StorageLayer;
use crate::providers::utils::TempFileGuard;
use crate::traits::{
    ServerSideOperations, StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

const MAGIC_HEADER: &[u8; 4] = b"CLZ1";
const MODE_RAW: u8 = 0;
const MODE_LZ: u8 = 1;

/// Middleware layer that applies transparent lossless compression to uploaded files.
#[derive(Debug, Clone, Default)]
pub struct CompressionLayer {
    /// Minimum file size in bytes before compression is attempted (defaults to 64 bytes).
    pub min_size_bytes: usize,
}

impl CompressionLayer {
    pub fn new() -> Self {
        Self { min_size_bytes: 64 }
    }

    pub fn with_min_size(min_size_bytes: usize) -> Self {
        Self { min_size_bytes }
    }
}

impl StorageLayer for CompressionLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(CompressedBackend::new(backend, self.min_size_bytes))
    }
}

/// Storage backend wrapper performing transparent `CLZ1` compression and decompression.
pub struct CompressedBackend {
    inner: Arc<dyn StorageBackend>,
    min_size_bytes: usize,
}

impl CompressedBackend {
    pub fn new(inner: Arc<dyn StorageBackend>, min_size_bytes: usize) -> Self {
        Self {
            inner,
            min_size_bytes,
        }
    }

    /// Returns `true` if `path` has a pre-compressed media, archive, or encrypted extension
    /// where LZ77 compression provides no benefit and should be bypassed.
    pub fn is_precompressed_path(path: &str) -> bool {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        matches!(
            ext.as_deref(),
            Some(
                "jpg"
                    | "jpeg"
                    | "png"
                    | "gif"
                    | "webp"
                    | "heic"
                    | "heif"
                    | "mp4"
                    | "mov"
                    | "mkv"
                    | "webm"
                    | "avi"
                    | "mp3"
                    | "aac"
                    | "ogg"
                    | "flac"
                    | "zip"
                    | "gz"
                    | "tgz"
                    | "bz2"
                    | "xz"
                    | "7z"
                    | "zst"
                    | "rar"
                    | "pdf"
                    | "apk"
                    | "enc"
            )
        )
    }

    /// Compresses `input` into a `CLZ1`-framed buffer.
    /// Frame layout: `[b"CLZ1" (4B)][mode (1B)][orig_len u64 LE (8B)][payload...]`
    pub fn compress_bytes(input: &[u8], min_size: usize) -> Vec<u8> {
        let orig_len = input.len() as u64;
        if input.len() >= min_size {
            let lz = lz_compress(input);
            if lz.len() < input.len() {
                let mut out = Vec::with_capacity(13 + lz.len());
                out.extend_from_slice(MAGIC_HEADER);
                out.push(MODE_LZ);
                out.extend_from_slice(&orig_len.to_le_bytes());
                out.extend_from_slice(&lz);
                return out;
            }
        }

        let mut out = Vec::with_capacity(13 + input.len());
        out.extend_from_slice(MAGIC_HEADER);
        out.push(MODE_RAW);
        out.extend_from_slice(&orig_len.to_le_bytes());
        out.extend_from_slice(input);
        out
    }

    /// Decompresses a `CLZ1`-framed buffer, or returns `input` unchanged if not `CLZ1`-framed.
    pub fn decompress_bytes(input: &[u8]) -> Result<Vec<u8>, StorageError> {
        if input.len() < 13 || &input[0..4] != MAGIC_HEADER {
            // Transparent passthrough for unframed/legacy files
            return Ok(input.to_vec());
        }

        let mode = input[4];
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&input[5..13]);
        let orig_len = u64::from_le_bytes(len_bytes) as usize;
        let payload = &input[13..];

        match mode {
            MODE_RAW => {
                if payload.len() != orig_len {
                    return Err(StorageError::Provider {
                        message: format!(
                            "CLZ1 raw frame size mismatch: expected {}, got {}",
                            orig_len,
                            payload.len()
                        ),
                        status: None,
                    });
                }
                Ok(payload.to_vec())
            }
            MODE_LZ => {
                let decoded = lz_decompress(payload, orig_len)?;
                Ok(decoded)
            }
            other => Err(StorageError::Provider {
                message: format!("Unknown CLZ1 compression mode: {}", other),
                status: None,
            }),
        }
    }
}

/// Fast pure-Rust byte-aligned LZ77 + literal run compressor.
/// Token stream:
/// - `0x00..=0x7F` (tag `L`): literal run of `(tag + 1)` bytes (`1..=128`) following tag.
/// - `0x80..=0xFF` (tag `M`): back-reference match of length `(tag - 0x80 + 3)` (`3..=130`)
///   followed by `u16` little-endian backward offset (`1..=65535`).
fn lz_compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2 + 16);
    let mut i = 0;
    let mut lit_start = 0;
    let n = input.len();

    let flush_literals = |out: &mut Vec<u8>, slice: &[u8]| {
        for chunk in slice.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
    };

    // Hash table mapping 3-byte prefix to most recent index
    let mut hash_table = vec![usize::MAX; 4096];
    let hash3 = |b: &[u8]| -> usize {
        (((b[0] as usize) << 8) ^ ((b[1] as usize) << 4) ^ (b[2] as usize)) & 4095
    };

    while i + 3 <= n {
        let h = hash3(&input[i..i + 3]);
        let prev = hash_table[h];
        hash_table[h] = i;

        if prev != usize::MAX
            && i > prev
            && (i - prev) <= 65535
            && input[prev..prev + 3] == input[i..i + 3]
        {
            let offset = (i - prev) as u16;
            let max_len = (n - i).min(130);
            let mut match_len = 3;
            while match_len < max_len && input[prev + match_len] == input[i + match_len] {
                match_len += 1;
            }

            if i > lit_start {
                flush_literals(&mut out, &input[lit_start..i]);
            }

            let tag = 0x80u8 + (match_len - 3) as u8;
            out.push(tag);
            out.extend_from_slice(&offset.to_le_bytes());

            // Update hashes for intermediate positions
            for k in (i + 1)..(i + match_len).min(n.saturating_sub(2)) {
                hash_table[hash3(&input[k..k + 3])] = k;
            }

            i += match_len;
            lit_start = i;
        } else {
            i += 1;
        }
    }

    if lit_start < n {
        flush_literals(&mut out, &input[lit_start..n]);
    }

    out
}

fn lz_decompress(payload: &[u8], expected_len: usize) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::with_capacity(expected_len);
    let mut i = 0;

    while i < payload.len() {
        let tag = payload[i];
        i += 1;

        if tag < 0x80 {
            let lit_len = (tag as usize) + 1;
            if i + lit_len > payload.len() {
                return Err(StorageError::Provider {
                    message: "Corrupted CLZ1 literal run".into(),
                    status: None,
                });
            }
            out.extend_from_slice(&payload[i..i + lit_len]);
            i += lit_len;
        } else {
            let match_len = (tag - 0x80) as usize + 3;
            if i + 2 > payload.len() {
                return Err(StorageError::Provider {
                    message: "Corrupted CLZ1 match offset".into(),
                    status: None,
                });
            }
            let offset = u16::from_le_bytes([payload[i], payload[i + 1]]) as usize;
            i += 2;

            if offset == 0 || offset > out.len() {
                return Err(StorageError::Provider {
                    message: format!("Invalid CLZ1 backward offset {}", offset),
                    status: None,
                });
            }

            let start = out.len() - offset;
            for k in 0..match_len {
                let b = out[start + k];
                out.push(b);
            }
        }
    }

    if out.len() != expected_len {
        return Err(StorageError::Provider {
            message: format!(
                "CLZ1 decompressed length mismatch: expected {}, got {}",
                expected_len,
                out.len()
            ),
            status: None,
        });
    }

    Ok(out)
}

#[async_trait]
impl ServerSideOperations for CompressedBackend {
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.copy(from, to).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to).await
    }
}

#[async_trait]
impl StorageBackend for CompressedBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        Some(self)
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.inner.list(remote_path).await
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let local_str = local_path.to_string_lossy();
        if Self::is_precompressed_path(remote_path) || Self::is_precompressed_path(&local_str) {
            // Fast-path: stream pre-compressed media/archive directly unless it starts with CLZ1 magic
            let starts_with_magic = {
                use tokio::io::AsyncReadExt;
                if let Ok(mut f) = tokio::fs::File::open(local_path).await {
                    let mut prefix = [0u8; 4];
                    f.read_exact(&mut prefix).await.is_ok() && &prefix == MAGIC_HEADER
                } else {
                    false
                }
            };
            if !starts_with_magic {
                return self.inner.upload(local_path, remote_path).await;
            }
        }

        let raw = tokio::fs::read(local_path).await?;
        let compressed = Self::compress_bytes(&raw, self.min_size_bytes);

        let temp_guard = TempFileGuard::new("clz1_up");
        tokio::fs::write(temp_guard.path(), &compressed).await?;
        self.inner.upload(temp_guard.path(), remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let temp_guard = TempFileGuard::new("clz1_dl");
        self.inner.download(remote_path, temp_guard.path()).await?;
        let raw = tokio::fs::read(temp_guard.path()).await?;

        let decompressed = Self::decompress_bytes(&raw)?;
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(local_path, &decompressed).await?;
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.delete(remote_path).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.copy(from, to).await
    }

    fn capabilities(&self) -> StorageCapabilities {
        let mut caps = self.inner.capabilities();
        caps.remove(StorageCapabilities::RESUMABLE_UPLOAD);
        caps.remove(StorageCapabilities::CHANGE_CURSOR);
        caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::LayeredBackendExt;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[test]
    fn test_lz_compression_roundtrip_and_ratio() {
        let text =
            "CloudSync transparent compression layer reduces storage & bandwidth! ".repeat(50);
        let compressed = CompressedBackend::compress_bytes(text.as_bytes(), 64);
        assert!(
            compressed.len() < text.len() / 3,
            "Expected >3x compression on repetitive text, got {} vs {}",
            compressed.len(),
            text.len()
        );

        let decompressed = CompressedBackend::decompress_bytes(&compressed).unwrap();
        assert_eq!(decompressed, text.as_bytes());
    }

    #[tokio::test]
    async fn test_compressed_backend_layer_roundtrip() {
        let remote_dir = tempdir().unwrap();
        let local_dir = tempdir().unwrap();

        let sim = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "TestSim".into(),
        ));
        let backend = sim.clone().with_layer(CompressionLayer::new());

        let original_content =
            "Log entry: [INFO] Syncing file block across multi-cloud providers\n".repeat(40);
        let src_file = local_dir.path().join("app.log");
        tokio::fs::write(&src_file, original_content.as_bytes())
            .await
            .unwrap();

        backend.upload(&src_file, "logs/app.log").await.unwrap();

        // Check that the stored file on the underlying backend is actually smaller than original
        let stored_items = sim.list("logs").await.unwrap();
        assert_eq!(stored_items.len(), 1);
        assert!(stored_items[0].size < original_content.len() as u64);

        let dl_file = local_dir.path().join("restored.log");
        backend.download("logs/app.log", &dl_file).await.unwrap();
        let restored = tokio::fs::read_to_string(&dl_file).await.unwrap();
        assert_eq!(restored, original_content);
    }

    #[tokio::test]
    async fn test_compressed_backend_precompressed_media_bypass() {
        let remote_dir = tempdir().unwrap();
        let local_dir = tempdir().unwrap();

        let sim = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "TestSim".into(),
        ));
        let backend = sim.clone().with_layer(CompressionLayer::new());

        assert!(CompressedBackend::is_precompressed_path("photos/IMG_001.JPG"));
        assert!(CompressedBackend::is_precompressed_path("videos/clip.mp4"));
        assert!(CompressedBackend::is_precompressed_path("archive.zip"));
        assert!(!CompressedBackend::is_precompressed_path("notes/readme.txt"));

        // Upload a .jpg file; it should bypass CLZ1 framing and have exact byte-for-byte size on remote
        let jpeg_payload = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let src_jpg = local_dir.path().join("photo.jpg");
        tokio::fs::write(&src_jpg, &jpeg_payload).await.unwrap();

        backend
            .upload(&src_jpg, "photos/photo.jpg")
            .await
            .unwrap();

        let stored = sim.list("photos").await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].size, jpeg_payload.len() as u64);

        let dl_jpg = local_dir.path().join("downloaded.jpg");
        backend
            .download("photos/photo.jpg", &dl_jpg)
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&dl_jpg).await.unwrap(), jpeg_payload);
    }
}
