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
