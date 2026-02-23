use std::time::Duration;

use bytes::BytesMut;
use futures::StreamExt;
use reqwest::Client;
use rmcp::model::Tool;
use serde_json::json;

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
#[derive(Clone)]
pub struct HttpBackend {
    client: Client,
    url: String,
    timeout: Duration,
    max_retries: u32,
    auth_secret: Option<String>,
}

impl HttpBackend {
    /// Create a new HttpBackend from a shared client and backend config.
    ///
    /// Appends `/mcp` to the URL if not already present.
    pub fn new(client: Client, config: &BackendConfig, auth_secret: Option<String>) -> Self {
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
            auth_secret,
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
        let auth_secret = self.auth_secret.clone();

        retry_with_backoff(self.max_retries, move || {
            let client = client.clone();
            let url = url.clone();
            let body = body.clone();
            let auth_secret = auth_secret.clone();

            async move {
                let mut req = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .timeout(timeout);

                if let Some(ref secret) = auth_secret {
                    req = req.header("X-Sentinel-Auth", secret.as_str());
                }

                let response = req
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

/// Discover tools from an HTTP MCP backend by performing the MCP handshake.
///
/// Sends initialize -> notifications/initialized -> tools/list, then
/// extracts and returns the tool definitions.
pub async fn discover_tools(backend: &HttpBackend) -> anyhow::Result<Vec<Tool>> {
    // Step 1: Send initialize request
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "sentinel-gateway",
                "version": "0.1.0"
            }
        }
    });
    let init_body = serde_json::to_string(&init_req)?;
    let init_response = backend.send(&init_body).await?;
    tracing::debug!(url = %backend.url(), response = %init_response, "MCP initialize response");

    // Step 2: Send notifications/initialized (fire and forget)
    let initialized_notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let notif_body = serde_json::to_string(&initialized_notif)?;
    if let Err(e) = backend.send(&notif_body).await {
        tracing::debug!(error = %e, "notifications/initialized send failed (expected for some backends)");
    }

    // Step 3: Send tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let list_body = serde_json::to_string(&list_req)?;
    let list_response = backend.send(&list_body).await?;
    tracing::debug!(url = %backend.url(), response = %list_response, "MCP tools/list response");

    // Parse tools from response
    let parsed: serde_json::Value = serde_json::from_str(&list_response)?;
    let tools_value = parsed
        .get("result")
        .and_then(|r| r.get("tools"))
        .ok_or_else(|| anyhow::anyhow!("No tools in tools/list response"))?;

    let tools: Vec<Tool> = serde_json::from_value(tools_value.clone())?;
    tracing::info!(
        url = %backend.url(),
        count = tools.len(),
        "Discovered tools from backend"
    );

    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{BackendConfig, BackendType};
    use std::collections::HashMap;

    fn test_config() -> BackendConfig {
        BackendConfig {
            name: "test".to_string(),
            backend_type: BackendType::Http,
            url: Some("http://localhost:3000".to_string()),
            command: None,
            args: vec![],
            env: HashMap::new(),
            timeout_secs: 60,
            retries: 3,
            restart_on_exit: false,
            max_restarts: 5,
            health_interval_secs: 300,
            circuit_breaker_threshold: 5,
            circuit_breaker_recovery_secs: 30,
        }
    }

    #[test]
    fn new_stores_auth_secret() {
        let client = build_http_client().unwrap();
        let backend = HttpBackend::new(client, &test_config(), Some("my-secret".to_string()));
        assert_eq!(backend.auth_secret.as_deref(), Some("my-secret"));
    }

    #[test]
    fn new_without_auth_secret() {
        let client = build_http_client().unwrap();
        let backend = HttpBackend::new(client, &test_config(), None);
        assert!(backend.auth_secret.is_none());
    }

    #[test]
    fn url_appends_mcp_path() {
        let client = build_http_client().unwrap();
        let backend = HttpBackend::new(client, &test_config(), None);
        assert_eq!(backend.url(), "http://localhost:3000/mcp");
    }
}
