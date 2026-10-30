//! Authentication and token provider abstractions (DIP).

use async_trait::async_trait;
use cloud_sync_core::StorageError;
use std::sync::Arc;

/// Abstract token provider for OAuth and API token lifecycle management.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    /// Retrieves a valid access token, utilizing cached tokens if fresh or refreshing if expired.
    async fn get_access_token(&self) -> Result<String, StorageError>;

    /// Invalidates any cache and forces an immediate token refresh.
    async fn force_refresh(&self) -> Result<String, StorageError>;
}

/// Static token provider providing a fixed access token (useful for test mocks or static API keys).
#[derive(Debug, Clone)]
pub struct StaticTokenProvider {
    token: String,
}

impl StaticTokenProvider {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

#[async_trait]
impl TokenProvider for StaticTokenProvider {
    async fn get_access_token(&self) -> Result<String, StorageError> {
        Ok(self.token.clone())
    }

    async fn force_refresh(&self) -> Result<String, StorageError> {
        Ok(self.token.clone())
    }
}

#[async_trait]
impl<T: TokenProvider + ?Sized> TokenProvider for Arc<T> {
    async fn get_access_token(&self) -> Result<String, StorageError> {
        (**self).get_access_token().await
    }

    async fn force_refresh(&self) -> Result<String, StorageError> {
        (**self).force_refresh().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_token_provider() {
        let provider = StaticTokenProvider::new("test_secret_token_abc");
        assert_eq!(provider.get_access_token().await.unwrap(), "test_secret_token_abc");
        assert_eq!(provider.force_refresh().await.unwrap(), "test_secret_token_abc");
    }

    #[tokio::test]
    async fn test_arc_token_provider_delegation() {
        let provider: Arc<dyn TokenProvider> = Arc::new(StaticTokenProvider::new("token_xyz"));
        assert_eq!(provider.get_access_token().await.unwrap(), "token_xyz");
    }
}

