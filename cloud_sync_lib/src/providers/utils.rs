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
    let body = res.text().await.unwrap_or_default();
    let detail = if body.trim().is_empty() {
        status.to_string()
    } else {
        body
    };
    StorageError::Provider(format!("Failed to {} on {}: {}", action, provider_name, detail))
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


