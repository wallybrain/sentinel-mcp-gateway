# Requirements: Sentinel Gateway

**Defined:** 2026-02-22
**Core Value:** Every MCP tool call passes through one governed point with auth, audit, and rate limiting — no ungoverned escape hatches.

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Authentication

- [x] **AUTH-01**: Gateway validates JWT tokens (HS256) on every incoming request, checking exp/iss/aud/jti claims
- [x] **AUTH-02**: Gateway rejects requests with missing, expired, or malformed tokens with JSON-RPC error response
- [x] **AUTH-03**: Gateway extracts role claims from JWT for downstream RBAC decisions

### Authorization

- [x] **AUTHZ-01**: Gateway enforces per-tool, per-role permissions defined in TOML config
- [x] **AUTHZ-02**: `tools/list` responses are filtered by caller's role — users only see tools they can call
- [x] **AUTHZ-03**: `tools/call` requests are rejected if caller's role lacks permission for the requested tool

### Routing

- [x] **ROUTE-01**: Gateway routes `tools/call` requests to the correct HTTP backend based on tool name
- [ ] **ROUTE-02**: Gateway routes `tools/call` requests to the correct stdio backend based on tool name
- [x] **ROUTE-03**: Gateway handles connection pooling, keep-alive, and configurable timeouts for HTTP backends
- [x] **ROUTE-04**: Gateway retries failed HTTP backend requests with exponential backoff and jitter

### stdio Backend Management

- [x] **STDIO-01**: Gateway spawns stdio backend processes from config (command + args + env vars)
- [x] **STDIO-02**: Gateway manages stdio backend lifecycle (health monitoring, crash detection)
- [x] **STDIO-03**: Gateway restarts crashed stdio backends with exponential backoff
- [x] **STDIO-04**: Gateway multiplexes concurrent JSON-RPC requests over a single stdio backend's stdin/stdout using request ID correlation
- [x] **STDIO-05**: Gateway cleanly terminates stdio backends on shutdown (process group kill, not just direct child)

### Protocol

- [x] **PROTO-01**: Gateway implements JSON-RPC 2.0 (request/response correlation, error objects, notifications)
- [x] **PROTO-02**: Gateway handles MCP initialize handshake and responds with merged capabilities
- [x] **PROTO-03**: Gateway handles `tools/list` by aggregating schemas from all backends into one catalog
- [x] **PROTO-04**: Gateway remaps JSON-RPC request IDs to prevent collisions between backends
- [x] **PROTO-05**: Gateway proxies SSE (text/event-stream) responses from backends without buffering
- [x] **PROTO-06**: Gateway accepts MCP requests via stdio transport (newline-delimited JSON-RPC on stdin/stdout)

### Audit

- [x] **AUDIT-01**: Gateway logs every tool call to Postgres with: timestamp, client identity, tool name, backend, request args (redactable), response status, latency
- [x] **AUDIT-02**: Gateway assigns a unique request ID to each tool call, included in all log entries
- [x] **AUDIT-03**: Audit logging is async and does not block request processing

### Rate Limiting

- [x] **RATE-01**: Gateway enforces per-client, per-tool rate limits using in-memory token bucket
- [x] **RATE-02**: Rate limit configuration is defined per-tool in TOML config with sensible defaults
- [x] **RATE-03**: Rate-limited requests receive JSON-RPC error with retry-after semantics

### Kill Switch

- [x] **KILL-01**: Gateway can disable individual tools via config (requests return JSON-RPC error)
- [x] **KILL-02**: Gateway can disable entire backends via config (all tools on that backend return error)
- [ ] **KILL-03**: Kill switch changes take effect via hot config reload without restart

### Health & Reliability

- [x] **HEALTH-01**: Gateway exposes `/health` endpoint (liveness — gateway process is running)
- [x] **HEALTH-02**: Gateway exposes `/ready` endpoint (readiness — at least one backend is reachable)
- [x] **HEALTH-03**: Gateway periodically pings backends and tracks their health status
- [x] **HEALTH-04**: Gateway implements circuit breaker per backend (open after N failures, half-open probe, close on success)
- [x] **HEALTH-05**: Gateway shuts down gracefully on SIGTERM (drain in-flight requests, terminate stdio children, flush audit logs)

### Configuration

- [x] **CONFIG-01**: All gateway behavior is configured via a single `sentinel.toml` file
- [x] **CONFIG-02**: Config includes: auth settings, backend definitions, role-to-tool mappings, rate limits, kill switches
- [ ] **CONFIG-03**: Gateway supports hot config reload via SIGHUP signal or file watch
- [x] **CONFIG-04**: Secrets (JWT key, Postgres password) are injected via environment variables, never in config file

### Observability

- [ ] **OBS-01**: Gateway exposes `/metrics` endpoint with Prometheus-compatible metrics
- [ ] **OBS-02**: Metrics include: request count, latency histogram, error rate, backend health, rate limit hits per tool
- [ ] **OBS-03**: Gateway validates tool call arguments against cached JSON schemas from `tools/list`
- [ ] **OBS-04**: Invalid arguments are rejected at the gateway with descriptive JSON-RPC error before reaching backend

### Deployment

- [x] **DEPLOY-01**: Gateway builds as a single Rust binary via `cargo build --release`
- [ ] **DEPLOY-02**: Gateway ships as a Docker image with multi-stage build (build + runtime)
- [ ] **DEPLOY-03**: Docker Compose file defines gateway + Postgres with health checks and restart policies
- [x] **DEPLOY-04**: Database schema migrations run automatically on gateway startup

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Authentication

- **AUTH-V2-01**: Full OAuth 2.1 support (PKCE, token refresh, authorization server)
- **AUTH-V2-02**: mTLS between gateway and backends

### Multi-tenancy

- **TENANT-01**: Per-tenant isolation for configs, rate limits, audit logs, and catalogs
- **TENANT-02**: Tenant-scoped tool discovery (different tenants see different tools)

### Extensibility

- **EXT-01**: Plugin system for pre/post request hooks
- **EXT-02**: OPA policy engine integration for complex authorization rules
- **EXT-03**: Streamable HTTP upstream transport (accept HTTP POST from clients, not just stdio)

### Observability

- **OBS-V2-01**: OpenTelemetry distributed tracing with span export
- **OBS-V2-02**: CLI management tool (`sentinel-cli status`, `sentinel-cli tools`, `sentinel-cli kill`)

### Deployment

- **DEPLOY-V2-01**: Blue/green and canary deployment support
- **DEPLOY-V2-02**: Schema versioning and tool version pinning

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| LLM provider proxying | Different product (AI gateway, not MCP gateway) |
| REST-to-MCP conversion | Backend servers handle this; gateway routes MCP natively |
| Web admin UI | Config files are the admin interface; UI adds frontend complexity for zero users |
| Caching layer | Tool calls are mostly writes or unique reads; caching adds staleness risk |
| A2A protocol | No current agent-to-agent use case; separate concern from tool governance |
| Redis | In-memory rate limiting sufficient for single-node; Postgres handles persistence |
| PgBouncer | Direct sqlx connection pool sufficient at this scale |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| AUTH-01 | Phase 4 | Complete |
| AUTH-02 | Phase 4 | Complete |
| AUTH-03 | Phase 4 | Complete |
| AUTHZ-01 | Phase 4 | Complete |
| AUTHZ-02 | Phase 4 | Pending |
| AUTHZ-03 | Phase 4 | Pending |
| ROUTE-01 | Phase 3 | Complete |
| ROUTE-02 | Phase 8 | Pending |
| ROUTE-03 | Phase 3 | Complete |
| ROUTE-04 | Phase 3 | Complete |
| STDIO-01 | Phase 8 | Pending |
| STDIO-02 | Phase 8 | Complete |
| STDIO-03 | Phase 8 | Complete |
| STDIO-04 | Phase 8 | Pending |
| STDIO-05 | Phase 8 | Pending |
| PROTO-01 | Phase 1 | Complete |
| PROTO-02 | Phase 2 | Complete |
| PROTO-03 | Phase 2 | Complete |
| PROTO-04 | Phase 1 | Complete |
| PROTO-05 | Phase 3 | Complete |
| PROTO-06 | Phase 2 | Complete |
| AUDIT-01 | Phase 5 | Complete |
| AUDIT-02 | Phase 5 | Complete |
| AUDIT-03 | Phase 5 | Complete |
| RATE-01 | Phase 6 | Pending |
| RATE-02 | Phase 6 | Pending |
| RATE-03 | Phase 6 | Pending |
| KILL-01 | Phase 6 | Pending |
| KILL-02 | Phase 6 | Pending |
| KILL-03 | Phase 9 | Pending |
| HEALTH-01 | Phase 7 | Complete |
| HEALTH-02 | Phase 7 | Complete |
| HEALTH-03 | Phase 7 | Complete |
| HEALTH-04 | Phase 7 | Complete |
| HEALTH-05 | Phase 7 | Complete |
| CONFIG-01 | Phase 1 | Complete |
| CONFIG-02 | Phase 1 | Complete |
| CONFIG-03 | Phase 9 | Pending |
| CONFIG-04 | Phase 1 | Complete |
| OBS-01 | Phase 9 | Pending |
| OBS-02 | Phase 9 | Pending |
| OBS-03 | Phase 9 | Pending |
| OBS-04 | Phase 9 | Pending |
| DEPLOY-01 | Phase 1 | Complete |
| DEPLOY-02 | Phase 10 | Pending |
| DEPLOY-03 | Phase 10 | Pending |
| DEPLOY-04 | Phase 5 | Complete |

**Coverage:**
- v1 requirements: 47 total
- Mapped to phases: 47
- Unmapped: 0

---
*Requirements defined: 2026-02-22*
*Last updated: 2026-02-22 after roadmap creation*
