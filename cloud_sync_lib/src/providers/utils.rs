use crate::traits::StorageError;

/// Refreshes OAuth2 access token by exchanging a long-lived refresh token.
///
/// Sends a form-encoded POST request to the provider's token validation URL and extracts the token from the response.
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
