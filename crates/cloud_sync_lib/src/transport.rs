//! Transport layer abstraction for decoupled HTTP / socket execution (DIP).

use async_trait::async_trait;
use cloud_sync_core::StorageError;
use std::sync::Arc;

/// Abstract HTTP transport for executing requests without direct coupling to concrete network clients.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Executes a pre-built reqwest request.
    async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response, StorageError>;

    /// Executes a reqwest RequestBuilder, converting build errors and network failures into `StorageError`.
    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, StorageError> {
        let request = builder.build().map_err(StorageError::Reqwest)?;
        self.execute(request).await
    }
}

/// Default production HTTP transport wrapping `reqwest::Client`.
#[derive(Clone, Debug)]
pub struct ReqwestHttpTransport {
    client: reqwest::Client,
}

impl ReqwestHttpTransport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

impl From<reqwest::Client> for ReqwestHttpTransport {
    fn from(client: reqwest::Client) -> Self {
        Self::new(client)
    }
}

#[async_trait]
impl HttpTransport for ReqwestHttpTransport {
    async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response, StorageError> {
        self.client
            .execute(request)
            .await
            .map_err(StorageError::Reqwest)
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, StorageError> {
        builder.send().await.map_err(StorageError::Reqwest)
    }
}

#[async_trait]
impl HttpTransport for reqwest::Client {
    async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response, StorageError> {
        self.execute(request).await.map_err(StorageError::Reqwest)
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, StorageError> {
        builder.send().await.map_err(StorageError::Reqwest)
    }
}

#[async_trait]
impl<T: HttpTransport + ?Sized> HttpTransport for Arc<T> {
    async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response, StorageError> {
        (**self).execute(request).await
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, StorageError> {
        (**self).send(builder).await
    }
}

/// In-memory mock HTTP transport for testing without network/sockets.
pub struct MockHttpTransport {
    handler: Box<dyn Fn(reqwest::Request) -> Result<reqwest::Response, StorageError> + Send + Sync>,
}

impl MockHttpTransport {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(reqwest::Request) -> Result<reqwest::Response, StorageError> + Send + Sync + 'static,
    {
        Self {
            handler: Box::new(handler),
        }
    }
}

#[async_trait]
impl HttpTransport for MockHttpTransport {
    async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response, StorageError> {
        (self.handler)(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{Response, StatusCode};

    #[tokio::test]
    async fn test_mock_http_transport_execution() {
        let transport = MockHttpTransport::new(|req| {
            assert_eq!(req.method(), reqwest::Method::GET);
            assert_eq!(req.url().as_str(), "https://api.example.com/test");

            let raw_res = Response::builder()
                .status(StatusCode::OK)
                .body("{\"status\": \"ok\"}")
                .unwrap();
            Ok(reqwest::Response::from(raw_res))
        });

        let client = reqwest::Client::new();
        let builder = client.get("https://api.example.com/test");
        let res = transport.send(builder).await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let text = res.text().await.unwrap();
        assert_eq!(text, "{\"status\": \"ok\"}");
    }

    #[tokio::test]
    async fn test_arc_http_transport_delegation() {
        let transport: Arc<dyn HttpTransport> = Arc::new(MockHttpTransport::new(|_| {
            let raw_res = Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body("")
                .unwrap();
            Ok(reqwest::Response::from(raw_res))
        }));

        let client = reqwest::Client::new();
        let builder = client.delete("https://api.example.com/item");
        let res = transport.send(builder).await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);
    }
}
