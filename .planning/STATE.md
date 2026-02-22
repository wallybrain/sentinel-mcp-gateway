# Project State: Sentinel Gateway

## Project Reference

| Field | Value |
|-------|-------|
| Core Value | Every MCP tool call passes through one governed point with auth, audit, and rate limiting |
| Current Focus | Phase 8 in progress -- supervisor + MCP handshake implemented (08-02 complete) |
| Language | Rust |
| Deployment | Docker Compose (gateway + Postgres) |

## Current Position

| Field | Value |
|-------|-------|
| Phase | 08-stdio-backend |
| Plan | 03 |
| Status | Phase 8 in progress (2/3 plans complete) |

**Overall Progress:**
```
Phase  1 [x] Foundation & Config (2/2 plans)
Phase  2 [x] MCP Protocol Layer (2/2 plans)
Phase  3 [x] HTTP Backend Routing (2/2 plans)
Phase  4 [x] Authentication & Authorization (2/2 plans)
Phase  5 [x] Audit Logging (2/2 plans)
Phase  6 [x] Rate Limiting & Kill Switch (2/2 plans)
Phase  7 [x] Health & Reliability (2/2 plans)
Phase  8 [~] stdio Backend Management (2/3 plans)
Phase  9 [ ] Observability & Hot Reload
Phase 10 [ ] Deployment & Integration
```

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 7/10 |
| Plans completed | 16/? |
| Requirements completed | 41/47 |
| Session count | 7 |

| Phase | Plan | Duration | Tasks | Files |
|-------|------|----------|-------|-------|
| 02 | 01 | 5min | 2 | 8 |
| 02 | 02 | 4min | 2 | 7 |
| 03 | 01 | 5min | 2 | 9 |
| 03 | 02 | 4min | 2 | 6 |
| 04 | 01 | 4min | 2 | 6 |
| 04 | 02 | 4min | 2 | 3 |
| 05 | 01 | 7min | 2 | 7 |
| 05 | 02 | 3min | 2 | 3 |
| 06 | 01 | 3min | 2 | 3 |
| 06 | 02 | 4min | 2 | 3 |
| 07 | 01 | 6min | 2 | 10 |
| 07 | 02 | 17min | 2 | 6 |
| 08 | 01 | 3min | 2 | 4 |
| 08 | 02 | 12min | 2 | 2 |

## Accumulated Context

### Key Decisions
- std::sync::Mutex<HashMap> over DashMap for rate limiter (single stdio transport = zero contention)
- Lazy refill on access (not background timer) for zero idle resource usage
- Optional audit_tx parameter (None for tests, Some when Postgres available)
- RBAC denials emit audit entries with status=denied and latency_ms=0
- request.params.clone() for handle_tools_call, original consumed by audit entry
- Runtime sqlx::query() instead of compile-time macros (no DATABASE_URL at build time)
- AuditEntry uses Clone derive for writer drain pattern
- Writer drains remaining entries on channel close (future-proofing for Phase 7)
- Rust for performance, learning, and single-binary deployment
- PostgreSQL for audit logs and config persistence
- Docker Compose for deployment (coexist with ContextForge during dev)
- Proprietary license (decide distribution later)
- stdio backend management is the key differentiator (no other gateway does this)
- rmcp 0.16 for protocol types only (not its server runtime)
- rmcp 0.16 requires `features = ["server"]` to compile -- default-features=false fails
- Bounded channels everywhere (no unbounded from wrapper pattern)
- JSON-RPC ID remapping from Phase 1 (architectural decision that cannot change later)
- Own JSON-RPC types instead of using jsonrpc-core for max serde control
- AtomicU64 counter starts at 1 (not 0) for gateway IDs -- avoids null-like zero values
- Lenient config loading (load_config_lenient) skips auth/postgres validation for early phases
- Tool collision resolution prefixes with backend_name__tool_name
- reqwest 0.12 (not 0.13) because 0.13 lacks rustls-tls feature
- Stub catalog fallback when no HTTP backends reachable (binary always starts)
- discover_tools() in backend/http.rs (collocated with HttpBackend)
- jsonwebtoken 10.x requires explicit rust_crypto feature (default-features panics at runtime)
- AuthError maps all variants to JSON-RPC -32001 for consistent error handling
- tools.execute implies tools.read (single is_tool_allowed function for both list and call)
- CallerIdentity passed to run_dispatch (not JwtValidator) for testability and separation of concerns
- JWT validation in main.rs, RBAC enforcement in gateway.rs
- AUTHZ_ERROR is -32003 (distinct from -32001 auth and -32002 not-initialized)
- Enforcement order: kill switch -> rate limit -> RBAC -> circuit breaker -> backend call
- Kill switch filters both tools/list and tools/call for consistency
- All rejection types (killed, rate_limited) emit audit entries with latency_ms=0
- Axum 0.8 for health HTTP server (separate from main MCP transport)
- AtomicU8 + AtomicU32 + Mutex<Option<Instant>> for lock-free circuit breaker state
- tower dev-dependency for Router::oneshot() in unit tests
- Clone audit_tx for dispatch, keep original for ordered shutdown drop
- Extract build_health_router() from run_health_server for test reuse
- HttpBackend derives Clone (reqwest::Client is Clone)
- std::sync::Mutex for StdioBackend pending map (matches rate limiter pattern, zero contention)
- process_group(0) + kill_on_drop(false) for explicit stdio process lifecycle management
- Individual env() calls to preserve parent environment inheritance
- Bounded mpsc(64) channel for stdin writes (backpressure on stalled child)
- Backoff jitter 0-50% of capped delay prevents synchronized restart storms
- Restart counter resets after 60s healthy operation (transient crashes don't accumulate)
- MCP handshake failure treated same as crash (kill child, backoff, restart)
- Tools channel sends backend clone for catalog/map updates after handshake

### Known Gotchas
- Rust builds require `dangerouslyDisableSandbox: true` (bwrap loopback error)
- Docker commands also need sandbox disabled
- rmcp 0.16 is pre-1.0 with 35% doc coverage -- wrap behind internal traits
- rmcp 0.16 default-features=false doesn't compile (unconditional server imports)
- npx creates process groups -- must kill entire group, not just parent
- Pre-install npm packages globally (never use npx in production)
- SSE client disconnect is NOT cancellation per MCP spec
- Auth bypass pitfall: RBAC must filter both tools/list AND tools/call

### Blockers
- None

### TODOs
- Execute Phase 8 plan 03 (gateway integration with Backend enum)

## Session Continuity

### Last Session
- **Date:** 2026-02-22
- **What happened:** Executed 08-02-PLAN.md -- supervisor with crash detection, exponential backoff, MCP handshake, 3 new tests (6 total)
- **Stopped at:** Completed 08-02-PLAN.md (Phase 8: 2/3 plans)
- **Next step:** Execute 08-03-PLAN.md (wire stdio into gateway dispatch with Backend enum)

---
*State initialized: 2026-02-22*
*Last updated: 2026-02-22T06:51Z*
