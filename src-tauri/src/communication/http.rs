//! HTTP/REST transport for the [`GaiaServerClient`] contract.
//!
//! Transport only: this client moves opaque JSON, applies auth headers and
//! reports status codes. It never inspects or shapes payloads semantically.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, Method, RequestBuilder};
use serde_json::Value;
use url::Url;

use super::client::{
    GaiaServerClient, HealthReport, ServerEventStream, ServerRequest, ServerResponse,
};
use super::CommunicationError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct HttpGaiaClient {
    base: Url,
    auth_token: Option<String>,
    http: Client,
}

impl HttpGaiaClient {
    pub fn new(base: Url, auth_token: Option<String>) -> Self {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builder is valid");
        Self {
            base,
            auth_token,
            http,
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, CommunicationError> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(|e| CommunicationError::InvalidUrl(e.to_string()))
    }

    fn authorize(&self, builder: RequestBuilder) -> RequestBuilder {
        match &self.auth_token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }
}

#[async_trait]
impl GaiaServerClient for HttpGaiaClient {
    async fn health(&self) -> Result<HealthReport, CommunicationError> {
        let started = Instant::now();
        let response = self
            .authorize(self.http.get(self.base.clone()))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
        match response {
            Ok(response) => Ok(HealthReport {
                reachable: true,
                latency_ms: Some(started.elapsed().as_millis() as u64),
                detail: Some(format!("HTTP {}", response.status().as_u16())),
            }),
            Err(e) => Ok(HealthReport {
                reachable: false,
                latency_ms: None,
                detail: Some(e.to_string()),
            }),
        }
    }

    async fn request(&self, request: ServerRequest) -> Result<ServerResponse, CommunicationError> {
        let url = self.endpoint(&request.path)?;
        let method = Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(|e| CommunicationError::Transport(e.to_string()))?;

        let mut builder = self.authorize(self.http.request(method, url));
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }

        let response = builder.send().await.map_err(map_transport)?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        if status >= 400 {
            return Err(CommunicationError::Server { status });
        }
        Ok(ServerResponse { status, body })
    }

    async fn subscribe_events(&self) -> Result<ServerEventStream, CommunicationError> {
        // Seam for the future WebSocket transport. The contract, channel type
        // and event envelope already exist; only the connection is missing.
        Err(CommunicationError::WebSocketUnavailable)
    }
}

fn map_transport(error: reqwest::Error) -> CommunicationError {
    CommunicationError::Transport(error.to_string())
}
