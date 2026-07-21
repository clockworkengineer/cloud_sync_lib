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
    let (access_token, _, _) = refresh_oauth2_token_details(client, auth_url, client_id, client_secret, refresh_token, provider_name).await?;
    Ok(access_token)
}

/// Refreshes OAuth2 access token and extracts updated refresh token and expires_in if returned.
pub async fn refresh_oauth2_token_details(
    client: &reqwest::Client,
    auth_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    provider_name: &str,
) -> Result<(String, Option<String>, Option<u64>), StorageError> {
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let res = client.post(auth_url)
        .form(&params)
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(translate_http_error(res, provider_name, "refresh_oauth2_token").await);
    }

    let json: serde_json::Value = res.json().await?;

    let access_token = json["access_token"].as_str().ok_or_else(|| {
        StorageError::Authentication(format!("Failed to retrieve {} access token: {:?}", provider_name, json))
    })?.to_string();

    let new_refresh_token = json["refresh_token"].as_str().map(|s| s.to_string());
    let expires_in = json["expires_in"].as_u64();

    Ok((access_token, new_refresh_token, expires_in))
}

pub type TokenRefreshCallback = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// Thread-safe manager to cache and refresh OAuth2 tokens automatically.
pub struct OAuthTokenManager {
    client: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: String,
    refresh_token: tokio::sync::RwLock<String>,
    provider_name: String,
    cache: tokio::sync::RwLock<Option<(String, std::time::Instant)>>,
    on_refresh: Option<TokenRefreshCallback>,
}

impl OAuthTokenManager {
    pub fn new(
        client: reqwest::Client,
        token_url: &str,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
        provider_name: &str,
    ) -> Self {
        Self::with_callback(client, token_url, client_id, client_secret, refresh_token, provider_name, None)
    }

    pub fn with_callback(
        client: reqwest::Client,
        token_url: &str,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
        provider_name: &str,
        on_refresh: Option<TokenRefreshCallback>,
    ) -> Self {
        Self {
            client,
            token_url: token_url.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            refresh_token: tokio::sync::RwLock::new(refresh_token.to_string()),
            provider_name: provider_name.to_string(),
            cache: tokio::sync::RwLock::new(None),
            on_refresh,
        }
    }

    pub async fn get_access_token(&self) -> Result<String, StorageError> {
        {
            let cache = self.cache.read().await;
            if let Some((ref token, expiry)) = *cache {
                if std::time::Instant::now() + std::time::Duration::from_secs(30) < expiry {
                    return Ok(token.clone());
                }
            }
        }

        let mut cache = self.cache.write().await;
        if let Some((ref token, expiry)) = *cache {
            if std::time::Instant::now() + std::time::Duration::from_secs(30) < expiry {
                return Ok(token.clone());
            }
        }

        let current_refresh_token = self.refresh_token.read().await.clone();

        let (token, new_refresh_token, expires_in) = refresh_oauth2_token_details(
            &self.client,
            &self.token_url,
            &self.client_id,
            &self.client_secret,
            &current_refresh_token,
            &self.provider_name,
        ).await?;

        if let Some(ref new_ref) = new_refresh_token {
            let mut ref_guard = self.refresh_token.write().await;
            *ref_guard = new_ref.clone();
            if let Some(ref cb) = self.on_refresh {
                cb(new_ref);
            }
        }

        let ttl = expires_in.unwrap_or(3600);
        let safety_margin = if ttl > 600 { 300 } else { ttl / 2 };
        let expiry = std::time::Instant::now() + std::time::Duration::from_secs(ttl.saturating_sub(safety_margin));

        *cache = Some((token.clone(), expiry));
        Ok(token)
    }
}

/// Unified helper to map an HTTP status code to `StorageError`.
pub fn translate_status_code_error(status_code: u16, provider_name: &str, action: &str, detail: Option<&str>) -> StorageError {
    let msg = match detail {
        Some(d) if !d.trim().is_empty() => d.to_string(),
        _ => format!("HTTP status {}", status_code),
    };

    match status_code {
        429 => StorageError::RateLimit {
            message: format!("Rate limit exceeded on {}: {}", provider_name, msg),
            retry_after: None,
        },
        404 => StorageError::NotFound(format!("Resource not found on {}: {}", provider_name, msg)),
        401 | 403 => StorageError::AuthenticationExpired(format!("Authentication expired or forbidden on {}: {}", provider_name, msg)),
        409 => StorageError::Conflict(format!("Conflict on {}: {}", provider_name, msg)),
        _ => StorageError::Provider {
            message: format!("Failed to {} on {}: {}", action, provider_name, msg),
            status: Some(status_code),
        },
    }
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
pub async fn translate_http_error(res: reqwest::Response, provider_name: &str, action: &str) -> StorageError {
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

    let mut err = translate_status_code_error(status.as_u16(), provider_name, action, Some(&detail));
    if let StorageError::RateLimit { retry_after: ref mut ra, .. } = err {
        if ra.is_none() {
            *ra = retry_after;
        }
    }
    err
}

/// Copies bytes from a reader to a writer using a standardized buffer size.
pub fn copy_buffered<R: std::io::Read, W: std::io::Write>(mut reader: R, mut writer: W) -> std::io::Result<u64> {
    let mut buffer = [0u8; 16384];
    let mut total_copied = 0;
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buffer[..bytes_read])?;
        total_copied += bytes_read as u64;
    }
    Ok(total_copied)
}

/// Helper to apply Bearer token authorization to an outgoing HTTP request.
pub fn apply_bearer_auth(req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    req.bearer_auth(token)
}

pub use cloud_sync_core::path::{normalize_remote_path, format_relative_path, format_absolute_path, strip_destination_prefix, url_encode, url_encode_path, get_parent_and_filename};

/// Generates standard `builder()`, `new()`, `timeout()`, and `custom_headers()` methods for a provider and its builder.
#[macro_export]
macro_rules! impl_provider_builder {
    ($provider:ident, $builder:ident, $creds:ty, absolute) => {
        $crate::impl_provider_builder!($provider, $builder, $creds);
        impl $provider {
            fn format_path<'a>(&self, remote_path: &'a str) -> std::borrow::Cow<'a, str> {
                $crate::providers::utils::format_absolute_path(remote_path, self.credentials.common.destination_folder.as_deref())
            }
        }
    };
    ($provider:ident, $builder:ident, $creds:ty, relative) => {
        $crate::impl_provider_builder!($provider, $builder, $creds);
        impl $provider {
            fn format_path<'a>(&self, remote_path: &'a str) -> std::borrow::Cow<'a, str> {
                $crate::providers::utils::format_relative_path(remote_path, self.credentials.common.destination_folder.as_deref())
            }
        }
    };
    ($provider:ident, $builder:ident, $creds:ty) => {
        impl $provider {
            /// Returns a new builder to configure the provider.
            pub fn builder(credentials: $creds) -> $builder {
                $builder::new(credentials)
            }

            /// Creates a new provider instance using the provided credentials.
            pub fn new(credentials: $creds) -> Self {
                Self::with_client_options(credentials, None, None)
            }
        }

        impl $builder {
            /// Configures the connection timeout.
            pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            /// Configures custom HTTP headers.
            pub fn custom_headers(mut self, headers: reqwest::header::HeaderMap) -> Self {
                self.custom_headers = Some(headers);
                self
            }
        }
    };
}

/// Helper macro to generate get_access_token delegate for OAuth providers.
#[macro_export]
macro_rules! impl_oauth_token_helper {
    ($provider:ident) => {
        impl $provider {
            async fn get_access_token(&self) -> Result<String, StorageError> {
                self.token_manager.get_access_token().await
            }
        }
    };
}

/// Centralized helper to build a standard reqwest::Client with proper pooling, timeout, and custom header settings.
pub fn build_http_client(
    timeout: Option<std::time::Duration>,
    custom_headers: Option<reqwest::header::HeaderMap>,
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout.unwrap_or(std::time::Duration::from_secs(600)))
        .pool_max_idle_per_host(10);
    if let Some(headers) = custom_headers {
        builder = builder.default_headers(headers);
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
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
        StorageError::ConnectionFailed(_) => true,
        StorageError::Provider { status: Some(status_code), .. } => {
            *status_code == 429 || *status_code == 502 || *status_code == 503 || *status_code == 504
        }
        StorageError::Provider { message, .. } => {
            message.contains("429") || message.contains("503") || message.contains("504") || message.contains("502")
        }
        _ => false,
    }
}

use std::sync::Mutex;

/// Global retry configuration.
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub initial_delay: std::time::Duration,
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: std::time::Duration::from_millis(500),
            multiplier: 2.0,
        }
    }
}

static GLOBAL_RETRY_CONFIG: Mutex<Option<RetryConfig>> = Mutex::new(None);

/// Sets the global retry configuration.
pub fn set_global_retry_config(config: RetryConfig) {
    if let Ok(mut lock) = GLOBAL_RETRY_CONFIG.lock() {
        *lock = Some(config);
    }
}

/// Retrieves the global retry configuration.
pub fn get_global_retry_config() -> RetryConfig {
    if let Ok(lock) = GLOBAL_RETRY_CONFIG.lock() {
        lock.clone().unwrap_or_default()
    } else {
        RetryConfig::default()
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
    let config = get_global_retry_config();
    let max_attempts = config.max_attempts;
    let mut attempt = 0;
    let mut delay = if cfg!(test) {
        std::time::Duration::from_millis(1)
    } else {
        config.initial_delay
    };
    let multiplier = config.multiplier;

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
                    delay = std::time::Duration::from_secs_f64(delay.as_secs_f64() * multiplier);
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

    #[tokio::test]
    async fn test_execute_with_retry_respects_retry_after() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use std::time::Instant;

        let server = MockServer::start().await;

        // Mock response returning 429 Too Many Requests with Retry-After: 1
        Mock::given(method("GET"))
            .and(path("/retry-test"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "1")
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        // Next attempt succeeds with 200 OK
        Mock::given(method("GET"))
            .and(path("/retry-test"))
            .respond_with(ResponseTemplate::new(200).set_body_string("success"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let request_url = format!("{}/retry-test", server.uri());

        let start = Instant::now();
        let result = execute_with_retry("test_retry_after", "get", || {
            let cl = client.clone();
            let url = request_url.clone();
            async move {
                let res = cl.get(&url).send().await.map_err(StorageError::Reqwest)?;
                if !res.status().is_success() {
                    return Err(translate_http_error(res, "test_retry_after", "get").await);
                }
                Ok("success")
            }
        }).await;

        let elapsed = start.elapsed();
        assert_eq!(result.unwrap(), "success");
        // It should have slept for at least 1 second due to the Retry-After: 1 header
        assert!(elapsed.as_millis() >= 950, "Should respect Retry-After delay, took {:?}", elapsed);
    }
}


