//! Utility functions for storage provider implementations.
//!
//! Provides shared routines for OAuth2 token refreshment and API error parsing.

use crate::traits::StorageError;

/// Refreshes OAuth2 access token by exchanging a long-lived refresh token.
///
/// Sends a form-encoded POST request to the provider's token validation URL and extracts the token from the response.
///
/// # Arguments
/// * `client` - The HTTP client to perform the request.
/// * `auth_url` - The provider's authorization / token exchange URL.
/// * `client_id` - The client ID registered with the provider.
/// * `client_secret` - The client secret registered with the provider.
/// * `refresh_token` - The long-lived refresh token.
/// * `provider_name` - The user-friendly name of the provider (for error reporting).
///
/// # Returns
/// The newly exchanged access token, or a `StorageError` if the operation fails.
pub async fn refresh_oauth2_token(
    client: &reqwest::Client,
    auth_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    provider_name: &str,
) -> Result<String, StorageError> {
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let res = client.post(auth_url)
        .form(&params)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let token = res["access_token"].as_str().ok_or_else(|| {
        StorageError::Authentication(format!("Failed to retrieve {} access token: {:?}", provider_name, res))
    })?;

    Ok(token.to_string())
}

/// Unified helper to parse error response from the provider REST API and map it to `StorageError`.
///
/// # Arguments
/// * `res` - The HTTP response containing the error.
/// * `provider_name` - The user-friendly name of the provider.
/// * `action` - The action that was being performed when the error occurred.
///
/// # Returns
/// A `StorageError` wrapping the details from the response.
pub async fn parse_response_error(res: reqwest::Response, provider_name: &str, action: &str) -> StorageError {
    let status = res.status();
    
    // Inspect Retry-After header
    let retry_after = res.headers().get(reqwest::header::RETRY_AFTER)
        .and_then(|val| val.to_str().ok())
        .and_then(|val_str| {
            if let Ok(secs) = val_str.parse::<u64>() {
                Some(std::time::Duration::from_secs(secs))
            } else {
                None
            }
        });

    let body = res.text().await.unwrap_or_default();
    let detail = if body.trim().is_empty() {
        status.to_string()
    } else {
        body
    };

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        StorageError::RateLimit {
            message: format!("Rate limit exceeded on {}: {}", provider_name, detail),
            retry_after,
        }
    } else if status == reqwest::StatusCode::NOT_FOUND {
        StorageError::NotFound(format!("Resource not found on {}: {}", provider_name, detail))
    } else {
        StorageError::Provider {
            message: format!("Failed to {} on {}: {}", action, provider_name, detail),
            status: Some(status.as_u16()),
        }
    }
}

/// Formats a relative remote path, incorporating an optional destination folder prefix.
pub fn format_relative_path(remote_path: &str, destination_folder: Option<&str>) -> String {
    let clean_path = remote_path.trim_start_matches('/');
    if let Some(dest_folder) = destination_folder {
        let clean_dest = dest_folder.trim_matches('/');
        if !clean_dest.is_empty() {
            if clean_path.is_empty() {
                return clean_dest.to_string();
            } else {
                return format!("{}/{}", clean_dest, clean_path);
            }
        }
    }
    clean_path.to_string()
}

/// Formats an absolute remote path starting with a slash, incorporating an optional destination folder prefix.
pub fn format_absolute_path(remote_path: &str, destination_folder: Option<&str>) -> String {
    let clean_path = remote_path.trim_start_matches('/');
    let mut full_path = String::new();
    if let Some(dest_folder) = destination_folder {
        let clean_dest = dest_folder.trim_matches('/');
        if !clean_dest.is_empty() {
            full_path.push('/');
            full_path.push_str(clean_dest);
        }
    }
    if !clean_path.is_empty() {
        full_path.push('/');
        full_path.push_str(clean_path);
    }
    full_path
}

/// Centralized helper to build a standard reqwest::Client with proper pooling and timeout settings.
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Creates a rate-limited reqwest::Body from a local file and returns the body along with its size.
pub async fn get_upload_body(
    local_path: &std::path::Path,
    limiter: Option<crate::rate_limit::TokenBucket>,
) -> Result<(reqwest::Body, u64), StorageError> {
    let file = tokio::fs::File::open(local_path).await?;
    let metadata = file.metadata().await?;
    let size = metadata.len();
    let reader = RateLimitedReader::new(file, limiter);
    let stream = ReaderStream::new(reader);
    let body = reqwest::Body::wrap_stream(stream);
    Ok((body, size))
}

/// Downloads a response body stream, limiting its rate, and writes it to a file.
pub async fn download_rate_limited(
    res: reqwest::Response,
    local_path: &std::path::Path,
    limiter: Option<crate::rate_limit::TokenBucket>,
) -> Result<(), StorageError> {
    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(local_path).await?;
    let byte_stream = res.bytes_stream();
    let mut rate_limited_stream = RateLimitedStream::new(byte_stream, limiter);
    use futures_util::stream::StreamExt;
    use tokio::io::AsyncWriteExt;
    
    while let Some(chunk_result) = rate_limited_stream.next().await {
        let chunk = chunk_result.map_err(StorageError::Reqwest)?;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(())
}

use crate::rate_limit::{RateLimitedReader, RateLimitedStream};
use tokio_util::io::ReaderStream;

/// Checks if a `StorageError` is transient and should trigger a retry.
pub fn is_transient_error(err: &StorageError) -> bool {
    match err {
        StorageError::RateLimit { .. } => true,
        StorageError::Reqwest(e) => {
            if e.is_timeout() || e.is_connect() {
                return true;
            }
            if let Some(status) = e.status() {
                status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error()
            } else {
                false
            }
        }
        StorageError::Provider { status: Some(status_code), .. } => {
            *status_code == 429 || *status_code == 502 || *status_code == 503 || *status_code == 504
        }
        StorageError::Provider { message, .. } => {
            message.contains("429") || message.contains("503") || message.contains("504") || message.contains("502")
        }
        _ => false,
    }
}

/// Executes an asynchronous operation with exponential backoff on transient errors.
pub async fn execute_with_retry<T, F, Fut>(
    provider_name: &str,
    action: &str,
    f: F,
) -> Result<T, StorageError>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<T, StorageError>> + Send,
{
    let mut attempt = 0;
    let max_attempts = 5;
    let mut delay = if cfg!(test) {
        std::time::Duration::from_millis(1)
    } else {
        std::time::Duration::from_millis(500)
    };

    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if is_transient_error(&e) && attempt < max_attempts - 1 {
                    attempt += 1;
                    
                    let sleep_duration = match &e {
                        StorageError::RateLimit { retry_after: Some(d), .. } => *d,
                        _ => delay,
                    };
                    
                    tracing::warn!(
                        "[{}] Transient error during {}: {}. Retrying in {:?} (attempt {}/{})",
                        provider_name,
                        action,
                        e,
                        sleep_duration,
                        attempt,
                        max_attempts
                    );
                    
                    tokio::time::sleep(sleep_duration).await;
                    if let StorageError::RateLimit { retry_after: Some(_), .. } = &e {
                        // Keep using the explicit retry-after window for rate-limiting sleep duration,
                        // but still double the default delay just in case we hit a non-rate-limit next
                        delay *= 2;
                    } else {
                        delay *= 2;
                    }
                    continue;
                }
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_execute_with_retry_success() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        
        let result = execute_with_retry("test", "op", || {
            let cnt = counter_clone.clone();
            async move {
                cnt.fetch_add(1, Ordering::SeqCst);
                Ok::<_, StorageError>("success")
            }
        }).await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_execute_with_retry_fail_non_transient() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = execute_with_retry("test", "op", || {
            let cnt = counter_clone.clone();
            async move {
                cnt.fetch_add(1, Ordering::SeqCst);
                Err::<(), StorageError>(StorageError::Authentication("Auth error".to_string()))
            }
        }).await;

        assert!(matches!(result, Err(StorageError::Authentication(_))));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_execute_with_retry_retry_then_success() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = execute_with_retry("test", "op", || {
            let cnt = counter_clone.clone();
            async move {
                let current = cnt.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    Err(StorageError::RateLimit { message: "Rate limit".to_string(), retry_after: None })
                } else {
                    Ok("success")
                }
            }
        }).await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(counter.load(Ordering::SeqCst), 3); // 0, 1 (retries) then 2 (success)
    }

    #[tokio::test]
    async fn test_execute_with_retry_max_attempts() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = execute_with_retry("test", "op", || {
            let cnt = counter_clone.clone();
            async move {
                cnt.fetch_add(1, Ordering::SeqCst);
                Err::<(), StorageError>(StorageError::RateLimit { message: "Rate limit".to_string(), retry_after: None })
            }
        }).await;

        assert!(matches!(result, Err(StorageError::RateLimit { .. })));
        assert_eq!(counter.load(Ordering::SeqCst), 5); // 5 attempts max
    }
}


