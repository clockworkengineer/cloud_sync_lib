use std::path::Path;
use std::io;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use ring::digest;

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use core::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Computes the SHA-256 checksum of a file at the specified path.
pub async fn compute_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut context = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 1024];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(hex_encode(context.finish().as_ref()))
}

/// Computes the MD5 checksum of a file at the specified path.
pub async fn compute_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut context = md5::Context::new();
    let mut buffer = [0; 1024];
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
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let mut bytes_read = 0;
        let mut block_context = digest::Context::new(&digest::SHA256);
        let block_size = 4 * 1024 * 1024;
        while bytes_read < block_size {
            let limit = std::cmp::min(buffer.len(), block_size - bytes_read);
            let n = file.read(&mut buffer[..limit]).await?;
            if n == 0 {
                break;
            }
            block_context.update(&buffer[..n]);
            bytes_read += n;
        }
        if bytes_read == 0 {
            break;
        }
        block_hashes.extend_from_slice(block_context.finish().as_ref());
    }
    let digest = digest::digest(&digest::SHA256, &block_hashes);
    Ok(hex_encode(digest.as_ref()))
}

/// Computes the SHA-1 checksum of a file at the specified path.
pub async fn compute_sha1(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut context = digest::Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    let mut buffer = [0; 1024];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(hex_encode(context.finish().as_ref()))
}
