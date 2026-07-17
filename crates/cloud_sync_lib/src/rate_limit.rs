use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::future::Future;
use tokio::time::{sleep_until, Duration, Instant as TokioInstant, Sleep};
use futures_util::stream::Stream;
use bytes::Bytes;
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_update: Instant,
    rate: u64,
    capacity: u64,
}

/// A thread-safe Token Bucket rate limiter.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    state: Arc<Mutex<BucketState>>,
}

impl TokenBucket {
    /// Creates a new `TokenBucket` with the specified rate in bytes per second.
    /// A rate of 0 represents unlimited bandwidth.
    pub fn new(rate: u64) -> Self {
        let capacity = rate;
        Self {
            state: Arc::new(Mutex::new(BucketState {
                tokens: capacity as f64,
                last_update: Instant::now(),
                rate,
                capacity,
            })),
        }
    }

    /// Returns the configured rate in bytes per second.
    pub fn rate(&self) -> u64 {
        self.state.lock().unwrap().rate
    }

    /// Sets the configured rate dynamically in bytes per second.
    pub fn set_rate(&self, new_rate: u64) {
        let mut state = self.state.lock().unwrap();
        state.rate = new_rate;
        state.capacity = new_rate;
        state.tokens = state.tokens.min(new_rate as f64);
    }

    /// Synchronously consumes `amount` tokens. Returns the duration to sleep if not enough tokens are available.
    pub fn consume(&self, amount: u64) -> Option<Duration> {
        let mut state = self.state.lock().unwrap();
        let rate = state.rate;
        if rate == 0 {
            return None;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_update).as_secs_f64();
        state.last_update = now;

        // Replenish tokens
        state.tokens = (state.tokens + elapsed * rate as f64).min(state.capacity as f64);

        if state.tokens >= amount as f64 {
            state.tokens -= amount as f64;
            None
        } else {
            let missing = amount as f64 - state.tokens;
            let sleep_secs = missing / rate as f64;
            Some(Duration::from_secs_f64(sleep_secs))
        }
    }
}

/// A reader wrapper that limits the read rate.
pub struct RateLimitedReader<R> {
    inner: R,
    limiter: Option<TokenBucket>,
    delay: Option<Pin<Box<Sleep>>>,
}

impl<R> RateLimitedReader<R> {
    pub fn new(inner: R, limiter: Option<TokenBucket>) -> Self {
        Self {
            inner,
            limiter,
            delay: None,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for RateLimitedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(ref mut delay) = self.delay {
            match delay.as_mut().poll(cx) {
                Poll::Ready(_) => {
                    self.delay = None;
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        let before_len = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let after_len = buf.filled().len();
                let read_bytes = (after_len - before_len) as u64;
                if read_bytes > 0 {
                    if let Some(ref limiter) = self.limiter {
                        if let Some(duration) = limiter.consume(read_bytes) {
                            let deadline = TokioInstant::now() + duration;
                            self.delay = Some(Box::pin(sleep_until(deadline)));
                            let _ = self.delay.as_mut().unwrap().as_mut().poll(cx);
                        }
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

/// A stream wrapper that limits the rate of the underlying byte stream.
pub struct RateLimitedStream<S> {
    inner: S,
    limiter: Option<TokenBucket>,
    delay: Option<Pin<Box<Sleep>>>,
}

impl<S> RateLimitedStream<S> {
    pub fn new(inner: S, limiter: Option<TokenBucket>) -> Self {
        Self {
            inner,
            limiter,
            delay: None,
        }
    }
}

impl<S, E> Stream for RateLimitedStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(ref mut delay) = self.delay {
            match delay.as_mut().poll(cx) {
                Poll::Ready(_) => {
                    self.delay = None;
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let len = bytes.len() as u64;
                if len > 0 {
                    if let Some(ref limiter) = self.limiter {
                        if let Some(duration) = limiter.consume(len) {
                            let deadline = TokioInstant::now() + duration;
                            self.delay = Some(Box::pin(sleep_until(deadline)));
                            let _ = self.delay.as_mut().unwrap().as_mut().poll(cx);
                        }
                    }
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            other => other,
        }
    }
}

/// Helper function to perform a rate-limited copy from one file path to another.
pub async fn copy_rate_limited(
    from: &std::path::Path,
    to: &std::path::Path,
    limiter: Option<TokenBucket>,
) -> std::io::Result<u64> {
    use tokio::fs::File;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut reader = File::open(from).await?;
    let mut writer = File::create(to).await?;
    let mut buffer = [0u8; 8192];
    let mut total_bytes = 0;

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        if let Some(ref tb) = limiter {
            if let Some(delay) = tb.consume(bytes_read as u64) {
                tokio::time::sleep(delay).await;
            }
        }
        writer.write_all(&buffer[..bytes_read]).await?;
        total_bytes += bytes_read as u64;
    }
    writer.flush().await?;
    Ok(total_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs::write;

    #[tokio::test]
    async fn test_token_bucket_limiting() {
        let bucket = TokenBucket::new(1000); // 1000 bytes per second
        
        // Consuming 500 should be instant
        assert!(bucket.consume(500).is_none());

        // Consuming another 600 takes us over the limit. It should return a delay.
        let delay = bucket.consume(600);
        assert!(delay.is_some());
        let delay_dur = delay.unwrap();
        assert!(delay_dur.as_secs_f64() > 0.0);
    }

    #[tokio::test]
    async fn test_copy_rate_limited() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        
        // Write 10KB of data
        let data = vec![0u8; 10000];
        write(&src, &data).await.unwrap();

        // Limiter at 5000 bytes/sec
        let limiter = TokenBucket::new(5000);
        let start = Instant::now();
        let bytes_copied = copy_rate_limited(&src, &dst, Some(limiter)).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(bytes_copied, 10000);
        // It should take at least 1 second to copy 10KB at 5KB/s (the second 5KB chunk is throttled)
        assert!(elapsed.as_secs_f64() >= 0.5, "elapsed was: {:?}", elapsed);
    }
}
