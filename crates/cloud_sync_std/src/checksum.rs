use std::path::Path;
use std::io;
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
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use std::io::Read;
        let mut file = File::open(&path)?;
        let mut context = digest::Context::new(&digest::SHA256);
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            context.update(&buffer[..n]);
        }
        Ok(hex_encode(context.finish().as_ref()))
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

/// Computes the MD5 checksum of a file at the specified path.
pub async fn compute_md5(path: &Path) -> io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use std::io::Read;
        let mut file = File::open(&path)?;
        let mut context = md5::Context::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            context.consume(&buffer[..n]);
        }
        Ok(format!("{:x}", context.compute()))
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

/// Computes the Dropbox-specific content_hash of a file at the specified path.
pub async fn compute_dropbox_hash(path: &Path) -> io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use std::io::Read;
        let mut file = File::open(&path)?;
        let mut block_hashes = Vec::new();
        let mut buffer = vec![0; 65536];
        loop {
            let mut bytes_read = 0;
            let mut block_context = digest::Context::new(&digest::SHA256);
            let block_size = 4 * 1024 * 1024;
            while bytes_read < block_size {
                let limit = std::cmp::min(buffer.len(), block_size - bytes_read);
                let n = file.read(&mut buffer[..limit])?;
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
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

/// Computes the SHA-1 checksum of a file at the specified path.
pub async fn compute_sha1(path: &Path) -> io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use std::io::Read;
        let mut file = File::open(&path)?;
        let mut context = digest::Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            context.update(&buffer[..n]);
        }
        Ok(hex_encode(context.finish().as_ref()))
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_checksums() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("hello.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"hello").unwrap();
        drop(file);

        // Test MD5
        let md5_val = compute_md5(&file_path).await.unwrap();
        assert_eq!(md5_val, "5d41402abc4b2a76b9719d911017c592");

        // Test SHA-256
        let sha256_val = compute_sha256(&file_path).await.unwrap();
        assert_eq!(sha256_val, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");

        // Test SHA-1
        let sha1_val = compute_sha1(&file_path).await.unwrap();
        assert_eq!(sha1_val, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

        // Test Dropbox hash
        let dbx_val = compute_dropbox_hash(&file_path).await.unwrap();
        let block_hash = ring::digest::digest(&ring::digest::SHA256, b"hello");
        let expected_dbx = hex_encode(ring::digest::digest(&ring::digest::SHA256, block_hash.as_ref()).as_ref());
        assert_eq!(dbx_val, expected_dbx);
    }
}
