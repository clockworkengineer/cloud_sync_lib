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
