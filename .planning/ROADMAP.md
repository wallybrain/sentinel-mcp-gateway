# Roadmap: Sentinel Gateway

**Created:** 2026-02-22
**Depth:** Comprehensive
**Phases:** 10
**Coverage:** 47/47 v1 requirements mapped

## Phases

- [x] **Phase 1: Foundation & Config** - Compiling binary with TOML config, JSON-RPC 2.0 types, and request ID remapping
- [x] **Phase 2: MCP Protocol Layer** - MCP lifecycle state machine, tool catalog aggregation, and stdio upstream transport
- [x] **Phase 3: HTTP Backend Routing** - Route tool calls to HTTP backends with connection pooling, retries, and SSE passthrough
- [x] **Phase 4: Authentication & Authorization** - JWT validation and per-tool per-role RBAC on every request
- [x] **Phase 5: Audit Logging** - Structured audit trail to Postgres with async writes and auto-migrations
- [x] **Phase 6: Rate Limiting & Kill Switch** - Per-client per-tool token bucket and per-tool/backend disable switches
- [x] **Phase 7: Health & Reliability** - Health endpoints, backend pinging, circuit breaker, and graceful shutdown
- [ ] **Phase 8: stdio Backend Management** - Spawn, supervise, and multiplex JSON-RPC over stdio child processes
- [ ] **Phase 9: Observability & Hot Reload** - Prometheus metrics, schema validation, and zero-downtime config reload
- [ ] **Phase 10: Deployment & Integration** - Docker image, Compose stack, and production cutover from ContextForge

## Phase Details

### Phase 1: Foundation & Config
**Goal**: A compiling Rust binary that loads configuration, defines JSON-RPC types, and establishes the architectural skeleton
**Depends on**: Nothing (first phase)
**Requirements**: PROTO-01, PROTO-04, CONFIG-01, CONFIG-02, CONFIG-04, DEPLOY-01
**Success Criteria** (what must be TRUE):
  1. Running `cargo build --release` produces a single binary with no errors or warnings
  2. The binary loads a `sentinel.toml` file and fails fast with a clear error if config is missing or malformed
  3. The config defines backend entries, role-to-tool mappings, rate limit settings, and kill switches in a typed schema
  4. Secrets (JWT key, Postgres password) are read from environment variables, never from the config file
  5. JSON-RPC request ID remapping logic exists and has unit tests proving no ID collision across backends
**Plans:** 2 plans
Plans:
- [x] 01-01-PLAN.md -- Scaffold Cargo project with typed TOML config system and integration tests
- [x] 01-02-PLAN.md -- JSON-RPC 2.0 types and request ID remapper (TDD)

### Phase 2: MCP Protocol Layer
**Goal**: The gateway speaks the MCP protocol -- handles initialize handshake, aggregates tool catalogs, and reads/writes stdio transport
**Depends on**: Phase 1
**Requirements**: PROTO-02, PROTO-03, PROTO-06
**Success Criteria** (what must be TRUE):
  1. The gateway responds to an MCP `initialize` request with a valid capabilities response (protocol version 2025-03-26)
  2. The gateway reads newline-delimited JSON-RPC from stdin and writes responses to stdout (stdio transport)
  3. The gateway aggregates `tools/list` from stub/mock backends into a single unified catalog
**Plans:** 2 plans
Plans:
- [x] 02-01-PLAN.md -- rmcp dependency, stdio transport, and MCP lifecycle state machine
- [x] 02-02-PLAN.md -- Tool catalog aggregation, dispatch loop, and end-to-end integration tests

### Phase 3: HTTP Backend Routing
**Goal**: Tool calls route to real HTTP backends (n8n, sqlite) with reliable connection handling and streaming support
**Depends on**: Phase 2
**Requirements**: ROUTE-01, ROUTE-03, ROUTE-04, PROTO-05
**Success Criteria** (what must be TRUE):
  1. A `tools/call` request for an n8n tool reaches the n8n MCP server and returns the correct response
  2. A `tools/call` request for a sqlite tool reaches the sqlite MCP server and returns the correct response
  3. SSE (text/event-stream) responses from backends are parsed incrementally and forwarded (MCP uses single-event SSE frames)
  4. A backend timeout or transient error triggers automatic retry with exponential backoff and jitter
  5. Idle HTTP connections are reused (connection pooling) and stale connections are cleaned up
**Plans:** 2 plans
Plans:
- [x] 03-01-PLAN.md -- HttpBackend struct, SSE parser, retry logic, and BackendError types
- [x] 03-02-PLAN.md -- Wire tools/call routing into dispatch loop and backend discovery into main.rs

### Phase 4: Authentication & Authorization
**Goal**: Every request is authenticated via JWT and authorized against per-tool per-role RBAC rules
**Depends on**: Phase 3
**Requirements**: AUTH-01, AUTH-02, AUTH-03, AUTHZ-01, AUTHZ-02, AUTHZ-03
**Success Criteria** (what must be TRUE):
  1. A request with a valid JWT token (correct HS256 signature, non-expired, correct iss/aud) is accepted
  2. A request with a missing, expired, or malformed token is rejected with a JSON-RPC error response
  3. `tools/list` returns only the tools the caller's role is permitted to use (unauthorized tools are invisible)
  4. `tools/call` for a tool the caller's role lacks permission for is rejected with a JSON-RPC error
  5. The same RBAC check function is used for both `tools/list` filtering and `tools/call` enforcement
**Plans:** 2/2 plans complete
Plans:
- [x] 04-01-PLAN.md -- JWT validator, RBAC module, and auth unit tests
- [x] 04-02-PLAN.md -- Wire auth into dispatch loop and update integration tests

### Phase 5: Audit Logging
**Goal**: Every tool call is recorded in Postgres with enough detail to answer "who did what, when, and what happened"
**Depends on**: Phase 4
**Requirements**: AUDIT-01, AUDIT-02, AUDIT-03, DEPLOY-04
**Success Criteria** (what must be TRUE):
  1. After a tool call completes, a row exists in Postgres with: timestamp, client identity, tool name, backend, request args, response status, and latency
  2. Every audit log entry includes a unique request UUID that traces the call end-to-end
  3. Audit writes are async -- a slow Postgres connection does not block or slow down tool call responses
  4. Database schema migrations run automatically when the gateway starts (no manual SQL required)
**Plans:** 2/2 plans complete
Plans:
- [x] 05-01-PLAN.md -- Audit module with PgPool, embedded migrations, AuditEntry struct, and async writer
- [x] 05-02-PLAN.md -- Wire audit into dispatch loop and main.rs startup sequence

### Phase 6: Rate Limiting & Kill Switch
**Goal**: The gateway can throttle abusive traffic per client per tool and instantly disable any tool or backend
**Depends on**: Phase 4
**Requirements**: RATE-01, RATE-02, RATE-03, KILL-01, KILL-02
**Success Criteria** (what must be TRUE):
  1. A client exceeding the configured rate limit for a tool receives a JSON-RPC error with retry-after semantics
  2. Rate limits are configurable per-tool in `sentinel.toml` with sensible defaults for unconfigured tools
  3. A tool marked as disabled in config returns a JSON-RPC error to callers (kill switch per tool)
  4. A backend marked as disabled in config causes all its tools to return JSON-RPC errors (kill switch per backend)
**Plans:** 2/2 plans complete
Plans:
- [x] 06-01-PLAN.md -- RateLimiter module with token bucket, error constants, and unit tests
- [x] 06-02-PLAN.md -- Wire kill switch and rate limiting into dispatch loop with integration tests

### Phase 7: Health & Reliability
**Goal**: The gateway reports its own health, monitors backend health, and shuts down cleanly without dropping requests
**Depends on**: Phase 3
**Requirements**: HEALTH-01, HEALTH-02, HEALTH-03, HEALTH-04, HEALTH-05
**Success Criteria** (what must be TRUE):
  1. `GET /health` returns 200 when the gateway process is running (liveness probe)
  2. `GET /ready` returns 200 when at least one backend is reachable, 503 otherwise (readiness probe)
  3. The gateway periodically pings each backend and tracks whether it is up or down
  4. A backend that fails N consecutive health checks is circuit-broken (requests fail fast without attempting the backend)
  5. On SIGTERM, the gateway drains in-flight requests, terminates stdio children, flushes audit logs, then exits
**Plans:** 2/2 plans complete
Plans:
- [x] 07-01-PLAN.md -- Health module with axum server, background checker, and circuit breaker
- [x] 07-02-PLAN.md -- Wire health, circuit breaker, and graceful shutdown into dispatch loop and main.rs

### Phase 8: stdio Backend Management
**Goal**: The gateway governs stdio-based MCP servers (context7, firecrawl, exa, playwright, sequential-thinking) -- the unique differentiator
**Depends on**: Phase 7
**Requirements**: STDIO-01, STDIO-02, STDIO-03, STDIO-04, STDIO-05, ROUTE-02
**Success Criteria** (what must be TRUE):
  1. The gateway spawns stdio backend processes from config (command, args, env vars) on startup
  2. A `tools/call` request for a stdio-backed tool (e.g., context7) routes correctly and returns the response
  3. Multiple concurrent requests to the same stdio backend are multiplexed over its single stdin/stdout using request ID correlation
  4. A crashed stdio backend is detected and restarted with exponential backoff
  5. On gateway shutdown, all stdio child processes are terminated cleanly (entire process group, not just direct child)
**Plans:** 3 plans
Plans:
- [x] 08-01-PLAN.md -- StdioBackend struct with multiplexer, process group spawn/kill, and unit tests
- [x] 08-02-PLAN.md -- Supervisor task with crash detection, exponential backoff restart, and MCP handshake
- [ ] 08-03-PLAN.md -- Wire stdio into gateway dispatch and main.rs with Backend enum and integration tests

### Phase 9: Observability & Hot Reload
**Goal**: The gateway exposes operational metrics, validates tool inputs, and supports zero-downtime config changes
**Depends on**: Phase 6, Phase 7
**Requirements**: OBS-01, OBS-02, OBS-03, OBS-04, CONFIG-03, KILL-03
**Success Criteria** (what must be TRUE):
  1. `GET /metrics` returns Prometheus-compatible metrics (request count, latency histogram, error rate, backend health, rate limit hits)
  2. Tool call arguments are validated against cached JSON schemas from `tools/list` before reaching the backend
  3. Invalid arguments are rejected at the gateway with a descriptive JSON-RPC error (never forwarded to backend)
  4. Sending SIGHUP or modifying the config file triggers a config reload without restarting the gateway
  5. Kill switch changes applied via hot reload take effect immediately on the next request
**Plans**: TBD

### Phase 10: Deployment & Integration
**Goal**: The gateway ships as a Docker image in a Compose stack and fully replaces ContextForge on the VPS
**Depends on**: Phase 8, Phase 9
**Requirements**: DEPLOY-02, DEPLOY-03
**Success Criteria** (what must be TRUE):
  1. `docker build` produces a minimal runtime image via multi-stage build (build stage + runtime stage)
  2. `docker compose up` starts the gateway + Postgres with health checks and restart policies
  3. All 7 MCP servers (2 HTTP + 5 stdio) are governed by Sentinel with identical responses to ContextForge
  4. ContextForge containers can be stopped and removed with no loss of MCP functionality
**Plans**: TBD

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation & Config | 2/2 | Complete | 2026-02-22 |
| 2. MCP Protocol Layer | 2/2 | Complete | 2026-02-22 |
| 3. HTTP Backend Routing | 2/2 | Complete | 2026-02-22 |
| 4. Authentication & Authorization | 2/2 | Complete    | 2026-02-22 |
| 5. Audit Logging | 2/2 | Complete    | 2026-02-22 |
| 6. Rate Limiting & Kill Switch | 2/2 | Complete    | 2026-02-22 |
| 7. Health & Reliability | 2/2 | Complete    | 2026-02-22 |
| 8. stdio Backend Management | 2/3 | In Progress | - |
| 9. Observability & Hot Reload | 0/? | Not started | - |
| 10. Deployment & Integration | 0/? | Not started | - |

---
*Roadmap created: 2026-02-22*
*Last updated: 2026-02-22*
