# Architecture Patterns

**Domain:** MCP Gateway (Rust, JSON-RPC 2.0, multi-transport)
**Researched:** 2026-02-22
**Confidence:** HIGH (based on MCP spec 2025-03-26, existing Rust wrapper analysis, ContextForge reference implementation, and Rust/Tower ecosystem docs)

## Recommended Architecture

```
                         Claude Code (or any MCP client)
                                    |
                                    | stdio (JSON-RPC, newline-delimited)
                                    v
                    +-------------------------------+
                    |       TRANSPORT LAYER         |
                    |  stdio_reader / stdio_writer  |
                    |  (reuse from Rust wrapper)    |
                    +-------------------------------+
                                    |
                                    | parsed JSON-RPC messages
                                    v
                    +-------------------------------+
                    |       PROTOCOL LAYER          |
                    |  MCP lifecycle state machine  |
                    |  initialize -> operational    |
                    +-------------------------------+
                                    |
                                    | validated MCP requests
                                    v
                    +-------------------------------+
                    |      MIDDLEWARE CHAIN          |
                    |  (Tower Layer stack)           |
                    |                               |
                    |  1. Auth (JWT HS256 validate)  |
                    |  2. RBAC (tool+role check)     |
                    |  3. Rate Limit (token bucket)  |
                    |  4. Kill Switch (tool/backend) |
                    |  5. Audit (log to Postgres)    |
                    +-------------------------------+
                                    |
                                    | authorized, rate-checked request
                                    v
                    +-------------------------------+
                    |        ROUTER LAYER           |
                    |  tool_name -> backend lookup   |
                    |  from merged tool catalog      |
                    +-------------------------------+
                                    |
                         +----------+----------+
                         |                     |
                         v                     v
              +------------------+   +------------------+
              |  HTTP BACKENDS   |   |  STDIO BACKENDS  |
              |  (reqwest POST)  |   |  (child process) |
              |                  |   |                  |
              |  n8n :3000       |   |  context7        |
              |  sqlite :3000   |   |  firecrawl       |
              |                  |   |  exa             |
              |                  |   |  seq-thinking    |
              |                  |   |  playwright      |
              +------------------+   +------------------+
                         |                     |
                         +----------+----------+
                                    |
                                    v
                    +-------------------------------+
                    |     RESPONSE PIPELINE         |
                    |  Audit completion log          |
                    |  JSON-RPC response assembly    |
                    |  stdio_writer output           |
                    +-------------------------------+
                                    |
                                    v
                              Claude Code
```

### Component Boundaries

| Component | Responsibility | Communicates With | Owns |
|-----------|---------------|-------------------|------|
| **Transport (stdio)** | Read stdin lines, write stdout lines, batched flush | Protocol Layer | stdin/stdout handles, line buffering |
| **Transport (HTTP upstream)** | Accept Streamable HTTP POST, return JSON or SSE | Protocol Layer | HTTP listener (axum), session IDs |
| **Protocol Layer** | MCP lifecycle state machine, JSON-RPC parsing, request ID correlation | Transport (up), Middleware (down) | Protocol state, capability negotiation |
| **Middleware Chain** | Auth, RBAC, rate limit, kill switch, audit | Protocol (up), Router (down), Postgres | JWT validation, RBAC rules, rate counters, audit records |
| **Router** | Map tool names to backends, merge tool catalogs | Middleware (up), Backends (down) | Tool registry, backend health state |
| **HTTP Backend Connector** | Forward requests to HTTP MCP servers via reqwest | Router | HTTP client, connection pool, SSE parsing |
| **Stdio Backend Manager** | Spawn/supervise child processes, multiplex stdin/stdout | Router | Child process handles, per-process channels |
| **Config System** | Load TOML config, provide typed access | All components | Backend definitions, RBAC rules, rate limits |
| **Health Monitor** | Periodic backend pings, readiness state | Router, Backends | Health state per backend |
| **Postgres Client** | Audit writes, config persistence, rate limit state | Middleware, Config | Connection pool (sqlx) |

### Data Flow

**Request path (happy path):**

```
1. stdin line arrives -> stdio_reader parses as JSON-RPC
2. Protocol layer checks lifecycle state:
   - If "initialize": negotiate capabilities, respond directly
   - If "tools/list": return merged catalog from Router
   - If "tools/call": pass to middleware chain
   - If "ping": respond directly (no middleware)
3. Auth middleware: validate JWT from config (stdio has no per-request auth;
   auth is validated once at gateway startup for stdio upstream)
4. RBAC middleware: check tool permission against role
5. Rate limit middleware: check/decrement token bucket
6. Kill switch middleware: check if tool or backend is disabled
7. Audit middleware: record request metadata to Postgres (async, non-blocking)
8. Router: lookup tool_name -> backend, dispatch
9. Backend connector: forward to HTTP or stdio backend
10. Response flows back through audit (record result status) -> stdout
```

**Key insight on auth for stdio upstream:** When Claude Code connects via stdio, there is no per-request Authorization header. Auth is implicit -- the gateway trusts its stdio parent. The JWT validation matters for the HTTP upstream transport (Streamable HTTP), where external clients connect. For stdio, RBAC still applies (configured per-tool), but JWT validation is skipped.

**Key insight on auth for HTTP upstream:** When the gateway exposes Streamable HTTP (for future clients or the Docker wrapper pattern), JWT validation runs on every POST. The `Mcp-Session-Id` header tracks sessions after `initialize`.

## Component Deep Dives

### 1. Transport Layer

**Two upstream transports, one internal representation:**

| Transport | When Used | Implementation |
|-----------|-----------|----------------|
| **stdio** | Claude Code launches gateway as child process | `tokio::io::stdin/stdout` with `BufReader`, newline-delimited JSON-RPC. Reuse `stdio_reader.rs` / `stdio_writer.rs` patterns from wrapper. |
| **Streamable HTTP** | Docker wrapper or remote clients POST to gateway | axum HTTP server. POST receives JSON-RPC, response is either JSON or SSE stream. `Mcp-Session-Id` header for session tracking. |

Both transports produce the same internal type: `McpRequest { id, method, params, transport_context }` where `transport_context` carries auth info (JWT from HTTP header, or `None` for stdio).

**Build order implication:** Build stdio first (simpler, matches current Claude Code setup), add HTTP upstream later.

### 2. Protocol Layer (MCP Lifecycle State Machine)

```
                     +-------------+
                     |   Created   |
                     +------+------+
                            |
                     initialize request
                            |
                     +------v------+
                     | Initializing|  <- negotiate capabilities
                     +------+------+
                            |
                     initialized notification
                            |
                     +------v------+
                     | Operational |  <- tools/list, tools/call, ping, resources/*
                     +------+------+
                            |
                     shutdown / transport close
                            |
                     +------v------+
                     |   Closed    |
                     +-------------+
```

**State machine rules (from MCP spec 2025-03-26):**
- No requests allowed before `initialize` completes (respond with error -32002)
- `initialize` response includes merged `capabilities` from all backends
- After `initialized` notification from client, enter Operational
- `tools/list` returns the merged catalog (aggregated from all backends)
- `tools/call` routes through middleware to the appropriate backend
- `ping` is handled directly (no routing needed)

**Capability merging:** Gateway aggregates capabilities from all backends. If any backend supports `tools`, the gateway advertises `tools`. Same for `resources`, `prompts`, `logging`.

### 3. Middleware Chain (Tower Layers)

Use Tower's `ServiceBuilder` to compose middleware as layers. Each layer wraps the next, forming an onion:

```rust
// Conceptual — actual types will be more complex
let service = ServiceBuilder::new()
    .layer(AuditLayer::new(pg_pool.clone()))       // outermost: sees request + response
    .layer(KillSwitchLayer::new(config.clone()))
    .layer(RateLimitLayer::new(config.clone()))
    .layer(RbacLayer::new(config.clone()))
    .layer(AuthLayer::new(jwt_secret.clone()))      // innermost auth check
    .service(RouterService::new(backends));
```

**Layer details:**

| Layer | Input | Output | State | Failure Mode |
|-------|-------|--------|-------|--------------|
| **AuthLayer** | `McpRequest` with optional JWT | `McpRequest` with validated claims | JWT secret (immutable) | JSON-RPC error -32001 (unauthorized) |
| **RbacLayer** | `McpRequest` with claims | Pass-through if allowed | RBAC config (reloadable) | JSON-RPC error -32001 (forbidden) |
| **RateLimitLayer** | `McpRequest` | Pass-through if under limit | Token bucket per (client, tool) | JSON-RPC error -32000 (rate limited) |
| **KillSwitchLayer** | `McpRequest` | Pass-through if enabled | Kill switch config (reloadable) | JSON-RPC error -32000 (tool disabled) |
| **AuditLayer** | `McpRequest` + response | Pass-through, async log | Postgres pool | Never fails request (log errors go to tracing) |

**Why Tower over hand-rolled middleware:** Tower layers are the standard Rust pattern for composable middleware. Axum is built on Tower, so if/when we add the HTTP upstream transport, the same middleware stack works for both transports. This is the key architectural win -- one middleware chain, two transports.

### 4. Router Layer

The Router maps tool names to backends. It owns the **merged tool catalog** -- the union of all tools from all backends.

```
Tool Registry (built at startup, refreshed on backend reconnect):

  "list_workflows"    -> HttpBackend("n8n", "http://mcp-n8n:3000")
  "sqlite_query"      -> HttpBackend("sqlite", "http://mcp-sqlite:3000")
  "resolve-library-id" -> StdioBackend("context7", child_pid=1234)
  "firecrawl_scrape"  -> StdioBackend("firecrawl", child_pid=1235)
  ...
```

**Tool discovery flow:**
1. At startup, gateway connects to each backend
2. Sends `initialize` to each, receives capabilities
3. Sends `tools/list` to each, receives tool schemas
4. Merges all tools into one catalog (detect name collisions, prefix if needed)
5. Caches the merged catalog
6. On `tools/list` from client, return the cached merged catalog

**Name collision strategy:** If two backends register the same tool name, prefix with backend name (`n8n__list_workflows`). Log a warning. This should not happen with current backends but must be handled.

### 5. Backend Connectors

#### HTTP Backend Connector

Reuse reqwest patterns from the Rust wrapper:
- `reqwest::Client` with connection pooling, `TCP_NODELAY`
- POST JSON-RPC to backend URL
- Handle both JSON responses and SSE streams
- Session ID tracking per backend (ArcSwap pattern)
- Timeout per request (configurable, default 60s)
- Retry with exponential backoff (max 3 retries)

#### Stdio Backend Manager

New component -- most complex part of the gateway.

```
StdioBackendManager
    |
    +-- StdioBackend("context7")
    |       |-- child: tokio::process::Child
    |       |-- stdin_tx: flume::Sender<JsonRpcRequest>  (writer task consumes)
    |       |-- stdout_rx: flume::Receiver<JsonRpcResponse>  (reader task produces)
    |       |-- pending: DashMap<JsonRpcId, oneshot::Sender<Response>>
    |       |-- state: AtomicU8 (Created/Initializing/Operational/Dead)
    |       |-- health: AtomicBool
    |
    +-- StdioBackend("firecrawl")
    |       |-- ...
    ...
```

**Per stdio backend, three async tasks:**
1. **Writer task:** Reads from `stdin_tx` channel, writes JSON-RPC lines to child stdin
2. **Reader task:** Reads lines from child stdout, parses JSON-RPC, looks up request ID in `pending` map, sends response via oneshot channel
3. **Supervisor task:** Monitors child process exit, restarts with backoff, re-initializes

**Request correlation for stdio backends:**
- Gateway sends `tools/call` to child stdin with a unique request ID
- Inserts `oneshot::Sender` into `pending` map keyed by that ID
- Reader task extracts response ID, finds the sender in `pending`, delivers response
- Timeout: if oneshot not fulfilled within N seconds, return error

**Lifecycle per stdio backend:**
1. Spawn child process (`npx @upstash/context7-mcp` etc.)
2. Send `initialize` request, await response
3. Send `initialized` notification
4. Send `tools/list`, cache tools
5. Mark as Operational
6. Route incoming `tools/call` requests to child

**Build order implication:** Stdio backend management is the hardest component. Build HTTP backends first (simpler, known pattern from wrapper), then stdio.

### 6. Config System

**TOML configuration** -- single file, human-readable:

```toml
[gateway]
listen = "127.0.0.1:9200"     # HTTP upstream (optional)
log_level = "info"
audit_enabled = true

[auth]
jwt_secret_env = "JWT_SECRET_KEY"  # env var name, not the secret itself
jwt_issuer = "sentinel-gateway"
jwt_audience = "sentinel-api"

[postgres]
url_env = "DATABASE_URL"
max_connections = 10

[[backends]]
name = "n8n"
type = "http"
url = "http://mcp-n8n:3000"
timeout = 60
retries = 3
health_interval = 300

[[backends]]
name = "sqlite"
type = "http"
url = "http://mcp-sqlite:3000"
timeout = 60

[[backends]]
name = "context7"
type = "stdio"
command = "npx"
args = ["@upstash/context7-mcp"]
restart_on_exit = true
max_restarts = 5

[[backends]]
name = "firecrawl"
type = "stdio"
command = "npx"
args = ["firecrawl-mcp"]
env = { FIRECRAWL_API_KEY_ENV = "FIRECRAWL_API_KEY" }

[rbac]
[rbac.roles.admin]
permissions = ["*"]

[rbac.roles.developer]
permissions = ["tools.read", "tools.execute"]

[rate_limits]
default_rpm = 1000
[rate_limits.per_tool]
execute_workflow = 10   # dangerous, limit heavily

[kill_switch]
disabled_tools = []
disabled_backends = []
```

### 7. Health Monitor

- Periodic pings to each backend (configurable interval, default 300s)
- HTTP backends: POST `{"jsonrpc":"2.0","method":"ping","id":1}`, expect response
- Stdio backends: same ping via stdin, with timeout
- Track consecutive failures; mark unhealthy after threshold (default 3)
- Unhealthy backends excluded from tool catalog until recovery
- Expose `/health` and `/ready` HTTP endpoints

## Patterns to Follow

### Pattern 1: Tower Layer Composition

**What:** Build each middleware concern as a Tower `Layer` that wraps an inner `Service`.
**When:** Any cross-cutting concern (auth, logging, rate limiting).
**Why:** Composable, testable in isolation, works with both stdio and HTTP transports.

```rust
use tower::{Layer, Service};

pub struct RbacLayer {
    config: Arc<RbacConfig>,
}

impl<S> Layer<S> for RbacLayer {
    type Service = RbacService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        RbacService { inner, config: self.config.clone() }
    }
}
```

### Pattern 2: Oneshot Channels for Request Correlation

**What:** Use `tokio::sync::oneshot` to correlate stdio requests with responses.
**When:** Sending a request to a stdio backend and waiting for the matching response.
**Why:** Zero-cost after fulfillment, type-safe, no polling needed.

```rust
let (tx, rx) = oneshot::channel();
pending_requests.insert(request_id.clone(), tx);
stdin_sender.send(request).await?;
let response = tokio::time::timeout(Duration::from_secs(60), rx).await??;
```

### Pattern 3: Typed JSON-RPC with serde

**What:** Strongly-typed request/response enums rather than raw `serde_json::Value`.
**When:** All JSON-RPC handling.
**Why:** Catch protocol errors at compile time. The wrapper's fast ID parser is useful for hot-path optimization but the gateway should parse fully for routing decisions.

```rust
#[derive(Deserialize)]
#[serde(tag = "method")]
enum McpMethod {
    #[serde(rename = "initialize")]
    Initialize { params: InitializeParams },
    #[serde(rename = "tools/list")]
    ToolsList { params: Option<ListParams> },
    #[serde(rename = "tools/call")]
    ToolsCall { params: ToolsCallParams },
    #[serde(rename = "ping")]
    Ping,
}
```

### Pattern 4: Graceful Shutdown with Signal Handling

**What:** Handle SIGTERM/SIGINT to cleanly shut down child processes and flush audit logs.
**When:** Always -- stdio child processes must be killed cleanly.
**Why:** Orphaned child processes leak resources. Unflushed audit logs lose data.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Unbounded Channels Without Backpressure

**What:** The existing wrapper uses `flume` unbounded channels everywhere.
**Why bad:** If a backend stalls, memory grows without bound. Under sustained load, OOM is possible.
**Instead:** Use bounded channels (`flume::bounded(1024)`) with backpressure. When the channel is full, the sender blocks, which propagates backpressure to the client. This is the correct behavior -- a slow backend should slow down the client, not accumulate unbounded memory.

### Anti-Pattern 2: Monolithic Request Handler

**What:** One giant `match` statement that handles auth, routing, RBAC, and audit inline.
**Why bad:** Untestable, unmaintainable, every change touches everything.
**Instead:** Tower middleware layers. Each concern is a separate, testable module.

### Anti-Pattern 3: Synchronous Audit Logging

**What:** Blocking on Postgres INSERT before returning the response.
**Why bad:** Adds latency to every tool call. Postgres hiccup blocks all requests.
**Instead:** Async audit via a bounded channel to a dedicated writer task. The audit layer sends the record and returns immediately. The writer task batches INSERTs for throughput. If the channel is full (Postgres down), drop oldest records and log a warning -- audit should never block the data plane.

### Anti-Pattern 4: Global Mutable State

**What:** Using `lazy_static!` or `once_cell` for shared mutable config.
**Why bad:** Hard to test, race conditions, no clear ownership.
**Instead:** Pass `Arc<Config>` through the Tower layer stack. Config reload uses `ArcSwap` (already proven in the wrapper) for lock-free reads with atomic updates.

### Anti-Pattern 5: Full JSON Deserialization on Hot Path

**What:** Parsing the entire JSON-RPC body just to extract the method name.
**Why bad:** Unnecessary allocation for routing decisions.
**Instead:** Use a two-phase parse: first extract `method` and `id` with a lightweight parser (the wrapper's actson-based fast parser), then full deserialization only when needed for the specific method handler.

## Build Order (Phase Dependencies)

The architecture has clear dependency layers. Build bottom-up:

```
Phase 1: Foundation
    Config (TOML) + Logging (tracing) + JSON-RPC types
    |
Phase 2: Stdio Transport + Protocol Layer
    stdio reader/writer + MCP state machine + hardcoded tool catalog
    |
Phase 3: HTTP Backend Connector
    reqwest client + SSE handling + single HTTP backend (n8n or sqlite)
    |
Phase 4: Router + Tool Discovery
    Multi-backend routing + tools/list aggregation + tool registry
    |
Phase 5: Middleware Chain
    Auth -> RBAC -> Rate Limit -> Kill Switch -> Audit (Tower layers)
    |
Phase 6: Stdio Backend Manager
    Child process spawn + stdin/stdout multiplexing + request correlation
    |
Phase 7: Postgres Integration
    Audit persistence + rate limit state + config persistence
    |
Phase 8: HTTP Upstream Transport (Streamable HTTP)
    axum server + session management + JWT validation per request
    |
Phase 9: Health Monitoring + Operational Polish
    Backend pings + /health + /ready + graceful shutdown + Docker Compose
```

**Rationale for this order:**

1. **Foundation first** -- every other component needs config and logging.
2. **Stdio transport early** -- this is how Claude Code connects. Get a working "echo" gateway immediately.
3. **HTTP backends before stdio backends** -- HTTP is the simpler connector (stateless, no process management). Connect to existing n8n/sqlite backends to prove routing works.
4. **Router after one backend works** -- add multi-backend routing once single-backend forwarding is proven.
5. **Middleware after routing** -- the middleware chain wraps the router. Need routing to work first so middleware has something to protect.
6. **Stdio backends are hard** -- child process lifecycle, crash recovery, request correlation. Defer until core gateway is solid.
7. **Postgres after middleware** -- audit and rate limiting need Postgres, but middleware can start with in-memory state (HashMap for rate limits, stderr for audit).
8. **HTTP upstream is optional for v1** -- stdio upstream covers the Claude Code use case. HTTP upstream enables the Docker wrapper pattern and future remote clients.
9. **Health and polish last** -- operational concerns after core functionality works.

**Critical path:** Phases 1-4 produce a working gateway that routes stdio requests to HTTP backends. This is the MVP -- it replaces ContextForge's core function.

## Scalability Considerations

| Concern | Current (1 user) | At 10 users | At 100 users |
|---------|-------------------|-------------|--------------|
| **Concurrency** | Single tokio runtime, more than sufficient | Same -- tokio handles thousands of tasks | May need multiple gateway instances behind load balancer |
| **Audit volume** | ~100 tool calls/day, trivial for Postgres | ~1000/day, still trivial | ~10K/day, batch INSERTs matter, consider partitioning |
| **Rate limiting** | In-memory HashMap sufficient | Same | Need shared state (Redis or Postgres) for multi-instance |
| **Stdio backends** | One set of child processes | Each gateway instance manages its own children | Shared nothing -- each instance spawns its own. Or: move to HTTP backends only |
| **Memory** | Target <100 MB for gateway binary | Same | Same per instance |
| **Tool catalog** | 19 tools + ~20 from stdio backends, cached | Same | Same -- catalog is static per config |

## Crate Recommendations

| Crate | Purpose | Why This One |
|-------|---------|-------------|
| `tokio` (1.x) | Async runtime | Industry standard, required by axum/reqwest/sqlx |
| `axum` (0.8+) | HTTP server (upstream Streamable HTTP) | Built on Tower, native middleware composition |
| `tower` (0.5+) | Middleware framework | The Rust standard for composable middleware |
| `reqwest` (0.13+) | HTTP client (downstream to backends) | Already proven in wrapper, rustls support |
| `sqlx` (0.8+) | Postgres client | Async, compile-time query checking, no ORM overhead |
| `serde` + `serde_json` | JSON serialization | Universal Rust JSON handling |
| `toml` | Config parsing | Native TOML support, clean API |
| `clap` (4.x) | CLI arguments | Already proven in wrapper |
| `tracing` | Structured logging | Already proven in wrapper, Tower integration |
| `jsonwebtoken` | JWT validation | Standard Rust JWT crate, HS256 support |
| `flume` | MPMC channels | Already proven in wrapper, bounded mode available |
| `arc-swap` | Lock-free config reload | Already proven in wrapper |
| `dashmap` | Concurrent HashMap | For pending request maps, rate limit counters |
| `mimalloc` | Allocator | Already proven in wrapper |

## Sources

- [MCP Specification 2025-03-26 - Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [MCP Specification 2025-03-26 - Basic Protocol](https://modelcontextprotocol.io/specification/2025-03-26/basic)
- [Why MCP Deprecated SSE for Streamable HTTP](https://blog.fka.dev/blog/2025-06-06-why-mcp-deprecated-sse-and-go-with-streamable-http/)
- [Tower Middleware for Auth and Logging in Axum](https://oneuptime.com/blog/post/2026-01-25-tower-middleware-auth-logging-axum-rust/view)
- [Tower crate documentation](https://docs.rs/tower)
- [Axum middleware documentation](https://docs.rs/axum/latest/axum/middleware/index.html)
- [Tokio process management](https://docs.rs/tokio/latest/tokio/process/index.html)
- [rust-mcp-sdk crate](https://crates.io/crates/rust-mcp-sdk)
- [MCP JSON-RPC Protocol Guide](https://mcpcat.io/guides/understanding-json-rpc-protocol-mcp/)
- Existing Rust wrapper at `/home/lwb3/mcp-context-forge/tools_rust/wrapper/` (890 lines, analyzed in `docs/RUST-WRAPPER-ANALYSIS.md`)
- ContextForge deployment at `/home/lwb3/mcp-context-forge/` (analyzed in `docs/CONTEXTFORGE-GATEWAY.md`)
- MCP topology documented in `docs/MCP-TOPOLOGY.md`
