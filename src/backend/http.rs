use std::time::Duration;

use bytes::BytesMut;
use futures::StreamExt;
use reqwest::Client;

use crate::config::types::BackendConfig;

use super::error::BackendError;
use super::retry::retry_with_backoff;
use super::sse::parse_sse_data;

/// Build a shared reqwest HTTP client with connection pooling.
pub fn build_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(5))
        .build()
}

/// HTTP backend that POSTs JSON-RPC to an MCP server.
pub struct HttpBackend {
    client: Client,
    url: String,
    timeout: Duration,
    max_retries: u32,
}

impl HttpBackend {
    /// Create a new HttpBackend from a shared client and backend config.
    ///
    /// Appends `/mcp` to the URL if not already present.
    pub fn new(client: Client, config: &BackendConfig) -> Self {
        let base = config
            .url
            .as_deref()
            .unwrap_or("http://localhost:3000")
            .trim_end_matches('/');

        let url = if base.ends_with("/mcp") {
            base.to_string()
        } else {
            format!("{base}/mcp")
        };

        Self {
            client,
            url,
            timeout: Duration::from_secs(config.timeout_secs),
            max_retries: config.retries,
        }
    }

    /// URL this backend targets (for logging).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Send a JSON-RPC request to the backend, with automatic retry on transient errors.
    pub async fn send(&self, json_rpc_body: &str) -> Result<String, BackendError> {
        let body = json_rpc_body.to_string();
        let client = self.client.clone();
        let url = self.url.clone();
        let timeout = self.timeout;

        retry_with_backoff(self.max_retries, move || {
            let client = client.clone();
            let url = url.clone();
            let body = body.clone();

            async move {
                let response = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .timeout(timeout)
                    .body(body)
                    .send()
                    .await
                    .map_err(BackendError::Request)?;

                let status = response.status();
                if !status.is_success() {
                    let body = response.text().await.unwrap_or_default();
                    return Err(BackendError::HttpStatus(status.as_u16(), body));
                }

                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                if content_type.contains("text/event-stream") {
                    Self::read_sse_response(response).await
                } else {
                    response.text().await.map_err(BackendError::Stream)
                }
            }
        })
        .await
    }

    /// Read an SSE response, accumulating chunks and parsing data lines.
    async fn read_sse_response(response: reqwest::Response) -> Result<String, BackendError> {
        let mut stream = response.bytes_stream();
        let mut buf = BytesMut::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(BackendError::Stream)?;
            buf.extend_from_slice(&chunk);
        }

        let raw = String::from_utf8_lossy(&buf);
        parse_sse_data(&raw).ok_or(BackendError::NoDataInSse)
    }
}
