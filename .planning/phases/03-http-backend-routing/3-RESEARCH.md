# Phase 3: HTTP Backend Routing - Research

**Researched:** 2026-02-22
**Domain:** HTTP client, SSE streaming, connection pooling, retry logic, MCP Streamable HTTP transport
**Confidence:** HIGH

## Summary

Phase 3 replaces the stub tool catalog with real HTTP backend communication. The gateway must POST JSON-RPC requests to the n8n and sqlite MCP backends (both running `@modelcontextprotocol/sdk` v1.25.3 with Streamable HTTP transport on Express) and handle their SSE-formatted responses. Both backends expose `POST /mcp` endpoints and **always return `Content-Type: text/event-stream`** with SSE framing (`event: message\ndata: {...}\n\n`), even for simple request-response interactions. There are no session IDs (both backends use `sessionIdGenerator: undefined`).

The existing Rust wrapper at `/home/lwb3/mcp-context-forge/tools_rust/wrapper/` already proves the core HTTP+SSE pattern with reqwest. The gateway needs to adapt this pattern for multi-backend routing, add connection pooling, retries with backoff, and integrate with the dispatch loop and ID remapper from Phases 1-2.

**Primary recommendation:** Build an `HttpBackend` struct wrapping a shared `reqwest::Client` that POSTs JSON-RPC to `/mcp`, parses SSE `data:` lines from the streaming response, and returns deserialized JSON-RPC responses. Use the existing ID remapper to avoid collisions. Add retry logic with exponential backoff + jitter for transient failures. Test against the real n8n and sqlite backends running on the Docker network.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ROUTE-01 | Gateway routes `tools/call` requests to correct HTTP backend based on tool name | ToolCatalog.route() already maps tool names to backend names; HttpBackend connects the backend name to a URL and sends the request |
| ROUTE-03 | Gateway handles connection pooling, keep-alive, and configurable timeouts for HTTP backends | reqwest::Client provides built-in connection pooling with `pool_max_idle_per_host()`, `pool_idle_timeout()`, `tcp_nodelay()`, and per-request `timeout()` |
| ROUTE-04 | Gateway retries failed HTTP backend requests with exponential backoff and jitter | Custom retry wrapper around reqwest POST; use `tokio::time::sleep` with exponential delay + random jitter; retry only on transient errors (timeout, connection refused, 5xx) |
| PROTO-05 | Gateway proxies SSE (text/event-stream) responses from backends without buffering | reqwest `bytes_stream()` processes chunks as they arrive; SSE line parsing strips `event:` and `data:` prefixes; no full-response buffering needed |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| reqwest | 0.13.x | HTTP client to MCP backends | Already proven in Rust wrapper. Connection pooling, streaming, rustls TLS, HTTP/2. `bytes_stream()` for SSE chunk processing. |
| bytes | 1.x | Zero-copy byte buffers | SSE line extraction without copying. `BytesMut` for buffer accumulation, `Bytes::freeze()` for output. Already in wrapper. |
| futures | 0.3.x | Stream utilities | `StreamExt::next()` for consuming `bytes_stream()`. Required for async iteration over response chunks. |
| tokio | 1.47.x | Async runtime + sleep/timeout | `tokio::time::sleep()` for retry backoff. `tokio::time::timeout()` for per-request deadline. Already a dependency. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rand | 0.9.x | Jitter for retry backoff | Add random jitter to exponential backoff to prevent thundering herd. Only needed in retry logic. |
| arc-swap | 1.x | Lock-free session ID storage (future) | If backends start returning `Mcp-Session-Id` headers. Currently not needed (both backends use `sessionIdGenerator: undefined`). Already a dependency in wrapper. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom retry logic | reqwest-retry / tower-http RetryLayer | reqwest-retry adds a dependency for a ~30 line function. Custom is clearer and allows MCP-specific retry decisions (e.g. never retry tools/call that mutated state). |
| Manual SSE parsing | eventsource-client / reqwest-eventsource | Both crates add dependencies for SSE parsing that is trivial in our case (strip `data:` prefix, split on `\n`). The wrapper already does this in ~50 lines. |
| Single reqwest::Client per backend | Shared client across all backends | reqwest::Client pools connections per host. A single shared client works fine because pooling is per-host internally. Simpler than per-backend clients. |

**Installation:**
```bash
# Add to Cargo.toml [dependencies]:
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json", "stream"] }
bytes = "1"
futures = "0.3"
rand = "0.9"
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── backend/
│   ├── mod.rs           # BackendConnector trait + HttpBackend struct
│   ├── http.rs          # HTTP backend implementation (reqwest POST + SSE parsing)
│   ├── sse.rs           # SSE line parser (extract_lines, strip data: prefix)
│   └── retry.rs         # Retry with exponential backoff + jitter
├── catalog/mod.rs       # (existing) ToolCatalog with route()
├── gateway.rs           # (modified) Dispatch loop with tools/call routing
├── protocol/
│   ├── jsonrpc.rs       # (existing) JSON-RPC types
│   └── id_remapper.rs   # (existing) ID remapping
└── config/types.rs      # (existing) BackendConfig with url, timeout, retries
```

### Pattern 1: HttpBackend Struct

**What:** A struct that holds the reqwest client, backend config, and sends JSON-RPC requests to the backend's `/mcp` endpoint.

**When to use:** Every `tools/call` and `tools/list` request that routes to an HTTP backend.

**Example:**
```rust
// Source: Adapted from existing Rust wrapper streamer_post.rs + streamer_send.rs
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use reqwest::Client;
use std::time::Duration;

pub struct HttpBackend {
    client: Client,
    url: String,          // e.g. "http://mcp-n8n:3000/mcp"
    timeout: Duration,
    max_retries: u32,
}

impl HttpBackend {
    pub fn new(client: Client, config: &BackendConfig) -> Self {
        let base_url = config.url.as_ref().expect("HTTP backend must have url");
        let url = if base_url.ends_with("/mcp") {
            base_url.clone()
        } else {
            format!("{}/mcp", base_url.trim_end_matches('/'))
        };
        Self {
            client,
            url,
            timeout: Duration::from_secs(config.timeout_secs),
            max_retries: config.retries,
        }
    }

    pub async fn send(&self, json_rpc_body: &str) -> Result<String, BackendError> {
        retry_with_backoff(self.max_retries, || async {
            let response = self.client
                .post(&self.url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(json_rpc_body.to_string())
                .timeout(self.timeout)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(BackendError::HttpStatus(status.as_u16(), body));
            }

            let is_sse = response.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.contains("text/event-stream"));

            if is_sse {
                self.read_sse_response(response).await
            } else {
                Ok(response.text().await?)
            }
        }).await
    }
}
```

### Pattern 2: SSE Response Parsing

**What:** Parse `text/event-stream` responses by streaming chunks, extracting complete lines, and stripping `data:` prefixes to get the JSON-RPC response.

**When to use:** All responses from both n8n and sqlite MCP backends (they always return SSE).

**Verified behavior (tested 2026-02-22):**
```
# Actual response from mcp-n8n backend:
HTTP/1.1 200 OK
Content-Type: text/event-stream

event: message
data: {"result":{...},"jsonrpc":"2.0","id":1}
```

**Example:**
```rust
// Source: Adapted from wrapper streamer_post.rs + mcp_workers_write.rs
async fn read_sse_response(&self, response: reqwest::Response) -> Result<String, BackendError> {
    let mut buffer = BytesMut::new();
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(BackendError::Stream)?;
        buffer.extend_from_slice(&chunk);
    }

    // Extract JSON-RPC from SSE data: lines
    let text = String::from_utf8_lossy(&buffer);
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(data) = trimmed.strip_prefix("data:") {
            let json_str = data.trim();
            if !json_str.is_empty() {
                return Ok(json_str.to_string());
            }
        }
    }

    Err(BackendError::NoDataInSse)
}
```

### Pattern 3: Retry with Exponential Backoff + Jitter

**What:** Retry transient failures (timeout, connection refused, 5xx) with exponential delay and random jitter.

**When to use:** Every HTTP request to a backend. Do NOT retry on 4xx errors (client errors, invalid request).

**Example:**
```rust
use rand::Rng;
use std::time::Duration;

async fn retry_with_backoff<F, Fut, T>(
    max_retries: u32,
    mut operation: F,
) -> Result<T, BackendError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, BackendError>>,
{
    let mut attempt = 0;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_retryable() && attempt < max_retries => {
                attempt += 1;
                let base_delay = Duration::from_millis(100 * 2u64.pow(attempt));
                let jitter = Duration::from_millis(
                    rand::rng().random_range(0..base_delay.as_millis() as u64 / 2)
                );
                let delay = base_delay + jitter;
                tracing::warn!(
                    attempt,
                    max_retries,
                    delay_ms = delay.as_millis() as u64,
                    error = %e,
                    "Retrying backend request"
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

### Pattern 4: Backend Discovery at Startup

**What:** During initialization, the gateway connects to each HTTP backend, sends `initialize`, then `tools/list`, and populates the ToolCatalog with real tools.

**When to use:** Gateway startup, replacing `create_stub_catalog()`.

**Flow:**
1. Build a shared `reqwest::Client` with connection pooling config
2. For each HTTP backend in config, create an `HttpBackend`
3. POST `initialize` request, parse SSE response
4. POST `notifications/initialized` (fire and forget)
5. POST `tools/list` request, parse SSE response
6. Extract tool definitions, register in ToolCatalog
7. Store `HttpBackend` in a `HashMap<String, HttpBackend>` keyed by backend name

### Anti-Patterns to Avoid

- **Buffering entire SSE stream before parsing:** The wrapper accumulates all chunks then extracts lines. For Phase 3 this is acceptable (responses are small), but structure the code so streaming can be added later. Do not build abstractions that assume the entire response fits in memory.
- **Creating a new reqwest::Client per request:** reqwest::Client is designed to be reused. Creating one per request defeats connection pooling. Create one at startup and share it.
- **Retrying non-idempotent tool calls blindly:** `execute_workflow` mutates state. Retrying it could execute the workflow twice. For v1, retry all requests (the MCP spec does not distinguish idempotent tools), but log a warning for non-idempotent operations. Add a TODO for per-tool retry policy.
- **Ignoring `event:` lines in SSE:** The backend sends `event: message\ndata: {...}`. Parsing only `data:` lines is correct. Do not try to handle `event:` semantically -- the JSON-RPC response is always in the `data:` line.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP connection pooling | Custom connection pool | reqwest::Client built-in pooling | reqwest handles keep-alive, idle timeout, max idle per host. Building custom pooling is error-prone. |
| TLS | OpenSSL bindings | reqwest + rustls (already configured) | Pure Rust TLS, no system dependency. |
| Byte buffer management | Manual Vec<u8> slicing | bytes::BytesMut + Bytes | Zero-copy splits, reference counting, memory-efficient for streaming. |
| Async streaming iteration | Manual poll loops | futures::StreamExt | `next().await` is the standard pattern for consuming async streams. |

## Common Pitfalls

### Pitfall 1: Backend URL Must Include `/mcp` Path
**What goes wrong:** Sending POST to `http://mcp-n8n:3000` instead of `http://mcp-n8n:3000/mcp` gets a 404 or unexpected response.
**Why it happens:** The sentinel.toml config defines `url = "http://mcp-n8n:3000"` (base URL), but the Streamable HTTP transport requires POSTing to `/mcp`.
**How to avoid:** The `HttpBackend::new()` constructor must append `/mcp` to the base URL if not already present. Validate at startup.
**Warning signs:** 404 responses or HTML error pages from Express.

### Pitfall 2: SSE Format Is Always Returned (Not Just for Streaming)
**What goes wrong:** Expecting `application/json` responses for simple requests like `initialize` or `tools/list`, then failing to parse the SSE framing.
**Why it happens:** Both n8n and sqlite backends use `StreamableHTTPServerTransport` which always returns `text/event-stream` with `event: message\ndata: {...}` framing, even for single responses.
**How to avoid:** Always check `Content-Type` header. If `text/event-stream`, parse SSE. If `application/json`, parse directly. Both paths must work, but in practice both backends always return SSE.
**Warning signs:** JSON parse errors on responses that contain `event:` and `data:` prefixes.

### Pitfall 3: No Session ID Management Needed (For Now)
**What goes wrong:** Implementing complex `Mcp-Session-Id` tracking when backends don't use it.
**Why it happens:** The MCP spec describes session IDs, but both backends use `sessionIdGenerator: undefined` (confirmed in code). No `Mcp-Session-Id` header is returned.
**How to avoid:** Design the code to support session IDs (check response headers, store if present), but don't make it mandatory. The `ArcSwap<Option<String>>` pattern from the wrapper is good for this.
**Warning signs:** None -- just unnecessary complexity if over-engineered now.

### Pitfall 4: ID Remapping Must Be Applied Before Sending to Backend
**What goes wrong:** Forwarding the client's original JSON-RPC ID to the backend. If two concurrent requests have the same ID (from different sessions or if the client reuses IDs), the gateway cannot correlate responses correctly.
**Why it happens:** The dispatch loop currently processes requests sequentially. When concurrency is added, ID collisions become possible.
**How to avoid:** Use the `IdRemapper` from Phase 1. Before sending to the backend: remap the client ID to a unique gateway ID. After receiving the response: restore the original client ID. This is already built -- just wire it into the HTTP send path.
**Warning signs:** Wrong responses returned to the client. Mismatched tool call results.

### Pitfall 5: Docker Network Resolution
**What goes wrong:** The gateway (running on the host or in a different Docker network) cannot resolve `mcp-n8n` or `mcp-sqlite` hostnames.
**Why it happens:** These container names resolve only within the `mcp-context-forge_mcpnet` Docker bridge network. If the gateway runs outside that network (or on the host), DNS resolution fails.
**How to avoid:** For development/testing, use the container's IP address or map ports to localhost. For production, the Sentinel gateway will join the same Docker network. Config should support both hostname and IP.
**Warning signs:** DNS resolution errors, connection refused.

### Pitfall 6: Notifications Have No `id` Field
**What goes wrong:** Sending `notifications/initialized` as a request (with `id` field) instead of a notification (without `id` field). The backend may reject it or behave unexpectedly.
**Why it happens:** Mixing up JSON-RPC requests and notifications. Notifications MUST NOT have an `id` field.
**How to avoid:** When sending `notifications/initialized` after the initialize handshake, omit the `id` field entirely from the JSON body. Do not expect a response.
**Warning signs:** Backend logs errors about unexpected `id` field, or sends back an error response.

## Code Examples

### Shared reqwest::Client Configuration
```rust
// Source: Adapted from wrapper http_client.rs
use reqwest::Client;
use std::time::Duration;

pub fn build_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(5))
        .build()
}
```

### BackendError Enum
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Backend returned HTTP {0}: {1}")]
    HttpStatus(u16, String),

    #[error("Stream error: {0}")]
    Stream(reqwest::Error),

    #[error("No data line found in SSE response")]
    NoDataInSse,

    #[error("Invalid JSON-RPC response: {0}")]
    InvalidResponse(String),
}

impl BackendError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Request(e) => e.is_timeout() || e.is_connect(),
            Self::HttpStatus(code, _) => *code >= 500,
            Self::Stream(_) => true,
            Self::NoDataInSse | Self::InvalidResponse(_) => false,
        }
    }
}
```

### Modified Dispatch Loop (tools/call routing)
```rust
// In gateway.rs -- the key change from Phase 2
"tools/call" => {
    if !is_notification {
        let id = request.id.clone().unwrap_or(JsonRpcId::Null);
        let params = request.params.as_ref();
        let tool_name = params
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str());

        match tool_name {
            Some(name) => match catalog.route(name) {
                Some(backend_name) => {
                    // Remap ID before sending to backend
                    let gateway_id = id_remapper.remap(id.clone(), backend_name);
                    let mut outbound = request.clone();
                    outbound.id = Some(JsonRpcId::Number(gateway_id));

                    let body = serde_json::to_string(&outbound)?;
                    match backends.get(backend_name) {
                        Some(backend) => match backend.send(&body).await {
                            Ok(response_str) => {
                                // Parse response, restore original ID
                                let mut response: JsonRpcResponse =
                                    serde_json::from_str(&response_str)?;
                                if let Some((original_id, _)) =
                                    id_remapper.restore(gateway_id)
                                {
                                    response.id = original_id;
                                }
                                send_response(&tx, &response).await;
                            }
                            Err(e) => { /* send JSON-RPC error */ }
                        },
                        None => { /* send backend not found error */ }
                    }
                }
                None => { /* send tool not found error */ }
            },
            None => { /* send invalid params error */ }
        }
    }
}
```

### Backend Initialization Sequence
```rust
// Discover tools from a real HTTP backend
pub async fn discover_tools(backend: &HttpBackend) -> Result<Vec<Tool>, BackendError> {
    // Step 1: Send initialize
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "sentinel-gateway", "version": "0.1.0"}
        },
        "id": 1
    });
    let init_response = backend.send(&init_request.to_string()).await?;
    tracing::info!(backend = %backend.url, "Backend initialized");

    // Step 2: Send initialized notification (no id, no response expected)
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    // Fire and forget -- ignore errors
    let _ = backend.send(&initialized.to_string()).await;

    // Step 3: Send tools/list
    let list_request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 2
    });
    let list_response = backend.send(&list_request.to_string()).await?;

    // Step 4: Parse tools from response
    let response: JsonRpcResponse = serde_json::from_str(&list_response)
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    let result = response.result
        .ok_or_else(|| BackendError::InvalidResponse("No result in tools/list response".into()))?;
    let tools_result: ListToolsResult = serde_json::from_value(result)
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;

    Ok(tools_result.tools)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SSE transport (deprecated) | Streamable HTTP | MCP spec 2025-03-26 | Both backends already use Streamable HTTP. Gateway sends POST, receives SSE or JSON. |
| reqwest 0.12 | reqwest 0.13 | 2025 | 0.13 updates to http 1.x, hyper 1.x. API is the same for our use case. |
| Per-connection auth header | Session-based auth | MCP spec 2025-03-26 | Not relevant yet -- backends have no sessions. Design for it but don't implement. |

**Current behavior of both backends (verified 2026-02-22):**
- Both use `@modelcontextprotocol/sdk` v1.25.3
- Both expose `POST /mcp` with Streamable HTTP transport
- Both always return `Content-Type: text/event-stream` with SSE framing
- Both use `sessionIdGenerator: undefined` (no sessions)
- Both run on Docker network `mcp-context-forge_mcpnet` at port 3000
- n8n backend: 9 tools (list/get/create/update/delete/activate/execute workflows, list/get executions)
- sqlite backend: 10 tools (query/execute/tables/schema/describe/databases/create_table/insert/backup/analyze)

## Open Questions

1. **Docker network access during development**
   - What we know: Backends are on `mcp-context-forge_mcpnet` at `mcp-n8n:3000` and `mcp-sqlite:3000`
   - What's unclear: How to route from the host-run gateway binary to these containers during dev/test
   - Recommendation: For integration tests, use port forwarding (`docker exec` or temporary port mapping). For unit tests, use wiremock to mock backends. Document both approaches.

2. **Concurrent request handling**
   - What we know: The dispatch loop is currently sequential (one request at a time via `while let Some(line) = rx.recv().await`)
   - What's unclear: Should Phase 3 make the dispatch loop concurrent (spawn per-request tasks) or keep it sequential?
   - Recommendation: Keep sequential for Phase 3. Concurrency adds complexity (shared mutable state for catalog, ID remapper contention). Add concurrency in a later phase when the HTTP path is proven correct.

3. **Notification forwarding to backends**
   - What we know: `notifications/initialized` must be sent after `initialize`. Other notifications may come from the client.
   - What's unclear: Should the gateway forward all client notifications to all backends, or only to relevant ones?
   - Recommendation: For Phase 3, only send `notifications/initialized` during discovery. Do not forward arbitrary client notifications to backends. Add notification forwarding in a later phase if needed.

## Sources

### Primary (HIGH confidence)
- Rust wrapper source: `/home/lwb3/mcp-context-forge/tools_rust/wrapper/` -- proven HTTP+SSE patterns with reqwest
- n8n MCP server source: `/home/lwb3/n8n-mcp-server/index.js` -- verified `StreamableHTTPServerTransport` with `sessionIdGenerator: undefined`
- sqlite MCP server source: `/home/lwb3/sqlite-mcp-server/index.js` -- same transport configuration
- Live testing (2026-02-22): Verified HTTP response headers and SSE format from `mcp-n8n` container
- Docker inspection (2026-02-22): Confirmed `MCP_TRANSPORT=http`, `MCP_PORT=3000` on both backends
- [MCP Specification 2025-03-26: Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) -- Streamable HTTP protocol definition
- [reqwest documentation](https://docs.rs/reqwest/latest/reqwest/) -- Client, connection pooling, streaming

### Secondary (MEDIUM confidence)
- [Why MCP Deprecated SSE for Streamable HTTP](https://blog.fka.dev/blog/2025-06-06-why-mcp-deprecated-sse-and-go-with-streamable-http/) -- SSE deprecation context
- [@modelcontextprotocol/sdk npm](https://www.npmjs.com/package/@modelcontextprotocol/sdk) -- v1.25.3 transport implementation details
- [reqwest connection pooling FAQ](https://webscraping.ai/faq/reqwest/is-there-a-way-to-customize-the-connection-pool-settings-in-reqwest) -- pool configuration options

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- reqwest is proven in the existing wrapper, same pattern applies
- Architecture: HIGH -- backend transport behavior verified by live testing against running containers
- Pitfalls: HIGH -- verified actual response format, confirmed no session IDs, confirmed SSE-always behavior
- Retry logic: MEDIUM -- standard pattern, but no empirical data on which errors are transient for these specific backends

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable -- reqwest and MCP SDK versions are unlikely to change in 30 days)
