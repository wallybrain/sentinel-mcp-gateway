# Project State: Sentinel Gateway

## Project Reference

| Field | Value |
|-------|-------|
| Core Value | Every MCP tool call passes through one governed point with auth, audit, and rate limiting |
| Current Focus | Phase 2 complete -- ready for Phase 3 (HTTP Backend Routing) |
| Language | Rust |
| Deployment | Docker Compose (gateway + Postgres) |

## Current Position

| Field | Value |
|-------|-------|
| Phase | 02-mcp-protocol-layer (complete) |
| Plan | 02-02 (complete) |
| Status | Phase 2 complete (2/2 plans) |

**Overall Progress:**
```
Phase  1 [x] Foundation & Config (2/2 plans)
Phase  2 [x] MCP Protocol Layer (2/2 plans)
Phase  3 [ ] HTTP Backend Routing
Phase  4 [ ] Authentication & Authorization
Phase  5 [ ] Audit Logging
Phase  6 [ ] Rate Limiting & Kill Switch
Phase  7 [ ] Health & Reliability
Phase  8 [ ] stdio Backend Management
Phase  9 [ ] Observability & Hot Reload
Phase 10 [ ] Deployment & Integration
```

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 2/10 |
| Plans completed | 4/? |
| Requirements completed | 11/47 |
| Session count | 4 |

| Phase | Plan | Duration | Tasks | Files |
|-------|------|----------|-------|-------|
| 02 | 01 | 5min | 2 | 8 |
| 02 | 02 | 4min | 2 | 7 |

## Accumulated Context

### Key Decisions
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
- Plan and execute Phase 3 (HTTP Backend Routing)

## Session Continuity

### Last Session
- **Date:** 2026-02-22
- **What happened:** Executed 02-02-PLAN.md -- tool catalog, dispatch loop, main.rs wiring, 11 new tests (41 total)
- **Stopped at:** Completed 02-02-PLAN.md (Phase 2 complete)
- **Next step:** Plan Phase 3 (HTTP Backend Routing)

---
*State initialized: 2026-02-22*
*Last updated: 2026-02-22T02:49Z*
