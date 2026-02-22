# Project State: Sentinel Gateway

## Project Reference

| Field | Value |
|-------|-------|
| Core Value | Every MCP tool call passes through one governed point with auth, audit, and rate limiting |
| Current Focus | Phase 5 in progress -- audit module foundation complete, dispatch integration next |
| Language | Rust |
| Deployment | Docker Compose (gateway + Postgres) |

## Current Position

| Field | Value |
|-------|-------|
| Phase | 05-audit-logging |
| Plan | 05-01 (complete) |
| Status | Phase 5 in progress (1/2 plans) |

**Overall Progress:**
```
Phase  1 [x] Foundation & Config (2/2 plans)
Phase  2 [x] MCP Protocol Layer (2/2 plans)
Phase  3 [x] HTTP Backend Routing (2/2 plans)
Phase  4 [x] Authentication & Authorization (2/2 plans)
Phase  5 [~] Audit Logging (1/2 plans)
Phase  6 [ ] Rate Limiting & Kill Switch
Phase  7 [ ] Health & Reliability
Phase  8 [ ] stdio Backend Management
Phase  9 [ ] Observability & Hot Reload
Phase 10 [ ] Deployment & Integration
```

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 4/10 |
| Plans completed | 9/? |
| Requirements completed | 21/47 |
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

## Accumulated Context

### Key Decisions
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
- Execute Phase 5 Plan 02 (wire audit into dispatch loop)

## Session Continuity

### Last Session
- **Date:** 2026-02-22
- **What happened:** Executed 05-01-PLAN.md -- audit module foundation with PgPool, migrations, AuditEntry struct, and async writer task
- **Stopped at:** Completed 05-01-PLAN.md (Phase 5, 1/2 plans)
- **Next step:** Execute 05-02-PLAN.md (wire audit into dispatch loop)

---
*State initialized: 2026-02-22*
*Last updated: 2026-02-22T04:15Z*
