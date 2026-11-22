//! Transparent Large-File Chunker Virtual Backend (Rclone `chunker` equivalent).
//!
//! Splits files exceeding `chunk_size_bytes` into numbered chunks (`<file>.cschunk_00001`, ...)
//! along with a JSON manifest (`<file>.cschunk_manifest`) storing original file size and per-chunk
//! SHA-256 hashes.
//! - Streams chunks in $O(1)$ memory using 64 KB buffers and `TempFileGuard`.
//! - `list()` hides raw `.cschunk_*` parts and synthesizes the logical original file entry.
//! - `download()` transparently reassembles chunks and verifies every chunk's SHA-256 digest.

use crate::providers::utils::TempFileGuard;
use crate::traits::{
    ServerSideOperations, StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MANIFEST_SUFFIX: &str = ".cschunk_manifest";
const PART_MIDFIX: &str = ".cschunk_";

/// Metadata for a single file chunk inside a [`ChunkManifest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkPartInfo {
    pub part_index: usize,
    pub remote_path: String,
    pub size: u64,
    pub sha256: String,
}

/// JSON manifest stored alongside chunked files on the underlying storage backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkManifest {
    pub original_path: String,
    pub total_size: u64,
    pub chunk_size: usize,
    pub chunks: Vec<ChunkPartInfo>,
}

/// Virtual storage backend that splits large files into content-verified chunks.
pub struct ChunkerBackend {
    inner: Arc<dyn StorageBackend>,
    chunk_size_bytes: usize,
}

impl ChunkerBackend {
    /// Wraps `inner` with transparent file chunking for files strictly larger than `chunk_size_bytes`.
    pub fn new(inner: Arc<dyn StorageBackend>, chunk_size_bytes: usize) -> Self {
        Self {
            inner,
            chunk_size_bytes: chunk_size_bytes.max(1024),
        }
    }

    pub fn chunk_size_bytes(&self) -> usize {
        self.chunk_size_bytes
    }

    fn manifest_path(remote_path: &str) -> String {
        format!("{}{}", remote_path, MANIFEST_SUFFIX)
    }

    fn part_path(remote_path: &str, index: usize) -> String {
        format!("{}{}{:05}", remote_path, PART_MIDFIX, index)
    }

    fn is_internal_chunk_part(path_str: &str) -> bool {
        if let Some(pos) = path_str.rfind(PART_MIDFIX) {
            let suffix = &path_str[pos + PART_MIDFIX.len()..];
            !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
        } else {
            false
        }
    }

    async fn fetch_manifest(
        &self,
        remote_path: &str,
    ) -> Result<Option<ChunkManifest>, StorageError> {
        let m_path = Self::manifest_path(remote_path);
        let temp_guard = TempFileGuard::new("cschunk_mf");
        match self.inner.download(&m_path, temp_guard.path()).await {
            Ok(()) => {
                let bytes = tokio::fs::read(temp_guard.path()).await?;
                let manifest: ChunkManifest =
                    serde_json::from_slice(&bytes).map_err(|e| StorageError::Provider {
                        message: format!("Corrupted chunk manifest for '{}': {}", remote_path, e),
                        status: None,
                    })?;
                Ok(Some(manifest))
            }
            Err(StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl ServerSideOperations for ChunkerBackend {
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let temp_guard = TempFileGuard::new("cschunk_cp");
        self.download(from, temp_guard.path()).await?;
        self.upload(temp_guard.path(), to).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if let Some(mut manifest) = self.fetch_manifest(from).await? {
            let mut new_chunks = Vec::with_capacity(manifest.chunks.len());
            for part in manifest.chunks {
                let new_part_remote = Self::part_path(to, part.part_index);
                self.inner
                    .rename(&part.remote_path, &new_part_remote)
                    .await?;
                new_chunks.push(ChunkPartInfo {
                    part_index: part.part_index,
                    remote_path: new_part_remote,
                    size: part.size,
                    sha256: part.sha256,
                });
            }
            manifest.original_path = to.to_string();
            manifest.chunks = new_chunks;

            let mf_bytes =
                serde_json::to_vec_pretty(&manifest).map_err(|e| StorageError::Provider {
                    message: e.to_string(),
                    status: None,
                })?;
            let temp_mf = TempFileGuard::new("cschunk_mv_mf");
            tokio::fs::write(temp_mf.path(), &mf_bytes).await?;
            self.inner
                .upload(temp_mf.path(), &Self::manifest_path(to))
                .await?;
            let _ = self.inner.delete(&Self::manifest_path(from)).await;
            return Ok(());
        }
        self.inner.rename(from, to).await
    }
}

#[async_trait]
impl StorageBackend for ChunkerBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        Some(self)
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        let raw_items = self.inner.list(remote_path).await?;
        let mut logical_items: HashMap<String, StorageItem> = HashMap::new();
        let mut part_sizes: HashMap<String, u64> = HashMap::new();

        for item in &raw_items {
            let p_str = item.path.to_string_lossy().replace('\\', "/");
            if Self::is_internal_chunk_part(&p_str) {
                if let Some(pos) = p_str.rfind(PART_MIDFIX) {
                    let base = p_str[..pos].to_string();
                    *part_sizes.entry(base).or_insert(0) += item.size;
                }
            }
        }

        for item in raw_items {
            let p_str = item.path.to_string_lossy().replace('\\', "/");
            if Self::is_internal_chunk_part(&p_str) {
                continue;
            }
            if let Some(orig_base) = p_str.strip_suffix(MANIFEST_SUFFIX) {
                let total_size = part_sizes.get(orig_base).copied().unwrap_or(item.size);
                logical_items.insert(
                    orig_base.to_string(),
                    StorageItem {
                        path: PathBuf::from(orig_base),
                        size: total_size,
                        modified: item.modified,
                        is_dir: false,
                        checksum: None,
                        permissions: item.permissions,
                    },
                );
            } else {
                logical_items.insert(p_str, item);
            }
        }

        let mut out: Vec<StorageItem> = logical_items.into_values().collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        let meta = tokio::fs::metadata(local_path).await?;
        let total_size = meta.len();

        if total_size <= self.chunk_size_bytes as u64 {
            if let Ok(Some(old_mf)) = self.fetch_manifest(remote_path).await {
                for part in old_mf.chunks {
                    let _ = self.inner.delete(&part.remote_path).await;
                }
                let _ = self.inner.delete(&Self::manifest_path(remote_path)).await;
            }
            return self.inner.upload(local_path, remote_path).await;
        }

        let _ = self.inner.delete(remote_path).await;

        let mut file = tokio::fs::File::open(local_path).await?;
        let mut chunks = Vec::new();
        let mut part_num = 1usize;
        let mut remaining = total_size;
        let mut buf = vec![0u8; 64 * 1024];

        while remaining > 0 {
            let part_target = remaining.min(self.chunk_size_bytes as u64);
            let p_remote = Self::part_path(remote_path, part_num);
            let temp_part = TempFileGuard::new("cschunk_up_part");

            {
                let mut part_file = tokio::fs::File::create(temp_part.path()).await?;
                let mut left_in_part = part_target as usize;
                while left_in_part > 0 {
                    let to_read = left_in_part.min(buf.len());
                    let n = file.read(&mut buf[..to_read]).await?;
                    if n == 0 {
                        break;
                    }
                    part_file.write_all(&buf[..n]).await?;
                    left_in_part -= n;
                }
                part_file.flush().await?;
            }

            let sha256 = crate::checksum::compute_sha256(temp_part.path()).await?;
            self.inner.upload(temp_part.path(), &p_remote).await?;

            chunks.push(ChunkPartInfo {
                part_index: part_num,
                remote_path: p_remote,
                size: part_target,
                sha256,
            });

            remaining -= part_target;
            part_num += 1;
        }

        let manifest = ChunkManifest {
            original_path: remote_path.to_string(),
            total_size,
            chunk_size: self.chunk_size_bytes,
            chunks,
        };

        let mf_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|e| StorageError::Provider {
                message: format!("Failed to serialize chunk manifest: {}", e),
                status: None,
            })?;
        let temp_mf = TempFileGuard::new("cschunk_up_mf");
        tokio::fs::write(temp_mf.path(), &mf_bytes).await?;
        self.inner
            .upload(temp_mf.path(), &Self::manifest_path(remote_path))
            .await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if let Some(manifest) = self.fetch_manifest(remote_path).await? {
            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let temp_out = TempFileGuard::new("cschunk_dl_assembled");
            {
                let mut out_file = tokio::fs::File::create(temp_out.path()).await?;
                for part in &manifest.chunks {
                    let temp_part = TempFileGuard::new("cschunk_dl_part");
                    self.inner
                        .download(&part.remote_path, temp_part.path())
                        .await?;

                    let actual_sha = crate::checksum::compute_sha256(temp_part.path()).await?;
                    if actual_sha != part.sha256 {
                        return Err(StorageError::Provider {
                            message: format!(
                                "Chunk integrity verification failed for '{}' part {}: expected SHA-256 {}, got {}",
                                remote_path, part.part_index, part.sha256, actual_sha
                            ),
                            status: None,
                        });
                    }

                    let mut part_reader = tokio::fs::File::open(temp_part.path()).await?;
                    crate::providers::utils::copy_async(&mut part_reader, &mut out_file).await?;
                }
                out_file.flush().await?;
            }
            tokio::fs::copy(temp_out.path(), local_path).await?;
            return Ok(());
        }

        self.inner.download(remote_path, local_path).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        if let Some(manifest) = self.fetch_manifest(remote_path).await? {
            for part in &manifest.chunks {
                let _ = self.inner.delete(&part.remote_path).await;
            }
            return self.inner.delete(&Self::manifest_path(remote_path)).await;
        }
        self.inner.delete(remote_path).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        ServerSideOperations::rename(self, from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        ServerSideOperations::copy(self, from, to).await
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
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_chunker_backend_split_list_reassemble_delete_and_compliance() {
        let remote_dir = tempdir().unwrap();
        let local_dir = tempdir().unwrap();

        let sim = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "ChunkedSim".into(),
        ));
        let chunker = ChunkerBackend::new(sim.clone(), 2048);

        let payload: Vec<u8> = (0..5500).map(|i| (i % 251) as u8).collect();
        let src_file = local_dir.path().join("video.mp4");
        tokio::fs::write(&src_file, &payload).await.unwrap();

        chunker.upload(&src_file, "media/video.mp4").await.unwrap();

        let raw_list = sim.list("media").await.unwrap();
        assert_eq!(raw_list.len(), 4);

        let logical_list = chunker.list("media").await.unwrap();
        assert_eq!(logical_list.len(), 1);
        assert_eq!(logical_list[0].path, PathBuf::from("media/video.mp4"));
        assert_eq!(logical_list[0].size, 5500);

        let dl_file = local_dir.path().join("reassembled.mp4");
        chunker.download("media/video.mp4", &dl_file).await.unwrap();
        let downloaded = tokio::fs::read(&dl_file).await.unwrap();
        assert_eq!(downloaded, payload);

        chunker.delete("media/video.mp4").await.unwrap();
        assert!(sim.list("media").await.unwrap().is_empty());

        // Run specification compliance suite on ChunkerBackend
        cloud_sync_core::testing::verify_backend_compliance(&chunker, local_dir.path())
            .await
            .unwrap();
    }
}
