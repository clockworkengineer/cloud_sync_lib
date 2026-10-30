//! High-performance byte buffer pool for reusable chunked uploads and downloads.
//!
//! Eliminates repetitive heap allocations during multipart and streaming file transfers.

use bytes::BytesMut;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

/// Thread-safe byte buffer pool.
#[derive(Clone, Debug)]
pub struct BufferPool {
    inner: Arc<BufferPoolInner>,
}

#[derive(Debug)]
struct BufferPoolInner {
    chunk_size: usize,
    max_pooled: usize,
    pool: Mutex<Vec<BytesMut>>,
}

impl BufferPool {
    /// Creates a new buffer pool where each buffer has at least `chunk_size` capacity,
    /// and at most `max_pooled` buffers are retained in memory.
    pub fn new(chunk_size: usize, max_pooled: usize) -> Self {
        Self {
            inner: Arc::new(BufferPoolInner {
                chunk_size,
                max_pooled,
                pool: Mutex::new(Vec::with_capacity(max_pooled)),
            }),
        }
    }

    /// Acquires a byte buffer from the pool, or allocates a new one if empty.
    pub fn acquire(&self) -> PooledBuffer {
        let mut pool = self.inner.pool.lock().unwrap();
        let buffer = pool
            .pop()
            .unwrap_or_else(|| BytesMut::with_capacity(self.inner.chunk_size));
        PooledBuffer {
            buffer,
            pool: Arc::clone(&self.inner),
        }
    }

    /// Returns the standard chunk size of this pool.
    pub fn chunk_size(&self) -> usize {
        self.inner.chunk_size
    }

    /// Returns current number of idle buffers available in the pool.
    pub fn idle_count(&self) -> usize {
        self.inner.pool.lock().unwrap().len()
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        // Standard 4 MB chunk size, retaining up to 8 idle buffers (32 MB max idle memory)
        Self::new(4 * 1024 * 1024, 8)
    }
}

/// RAII guard wrapping a pooled `BytesMut` buffer that automatically returns
/// the buffer to its owning pool on drop.
pub struct PooledBuffer {
    buffer: BytesMut,
    pool: Arc<BufferPoolInner>,
}

impl Deref for PooledBuffer {
    type Target = BytesMut;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let mut pool = self.pool.pool.lock().unwrap();
        if pool.len() < self.pool.max_pooled {
            let mut buf = std::mem::take(&mut self.buffer);
            buf.clear();
            pool.push(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_reuse() {
        let pool = BufferPool::new(1024, 2);
        assert_eq!(pool.idle_count(), 0);

        {
            let mut buf1 = pool.acquire();
            buf1.extend_from_slice(b"hello pool");
            assert_eq!(&buf1[..], b"hello pool");
            assert_eq!(pool.idle_count(), 0);
        }

        // Dropped buf1 returned to pool
        assert_eq!(pool.idle_count(), 1);

        {
            let mut buf2 = pool.acquire();
            assert_eq!(buf2.len(), 0); // Cleared on return
            buf2.extend_from_slice(b"second use");
            assert_eq!(&buf2[..], b"second use");
            assert_eq!(pool.idle_count(), 0);
        }

        assert_eq!(pool.idle_count(), 1);
    }

    #[test]
    fn test_buffer_pool_capacity_bound() {
        let pool = BufferPool::new(512, 1);
        let b1 = pool.acquire();
        let b2 = pool.acquire();
        drop(b1);
        assert_eq!(pool.idle_count(), 1);
        drop(b2);
        // Exceeds max_pooled (1), so second buffer is dropped without inflating memory
        assert_eq!(pool.idle_count(), 1);
    }
}

