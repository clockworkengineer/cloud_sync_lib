use std::path::Path;
use std::io;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use sha2::{Sha256, Digest};

/// Computes the SHA-256 checksum of a file at the specified path.
pub async fn compute_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Computes the MD5 checksum of a file at the specified path.
pub async fn compute_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut context = md5::Context::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        context.consume(&buffer[..n]);
    }
    Ok(format!("{:x}", context.compute()))
}

/// Computes the Dropbox-specific content_hash of a file at the specified path.
pub async fn compute_dropbox_hash(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut block_hashes = Vec::new();
    let mut buffer = vec![0; 4 * 1024 * 1024]; // 4MB block size
    loop {
        let mut bytes_read = 0;
        while bytes_read < buffer.len() {
            let n = file.read(&mut buffer[bytes_read..]).await?;
            if n == 0 {
                break;
            }
            bytes_read += n;
        }
        if bytes_read == 0 {
            break;
        }
        let mut hasher = Sha256::new();
        hasher.update(&buffer[..bytes_read]);
        block_hashes.extend_from_slice(&hasher.finalize());
    }
    let mut hasher = Sha256::new();
    hasher.update(&block_hashes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Computes the SHA-1 checksum of a file at the specified path.
pub async fn compute_sha1(path: &Path) -> io::Result<String> {
    use sha1::{Sha1, Digest};
    let mut file = File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}


