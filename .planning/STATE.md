# Project State: Sentinel Gateway

## Project Reference

| Field | Value |
|-------|-------|
| Core Value | Every MCP tool call passes through one governed point with auth, audit, and rate limiting |
| Current Focus | Phase 1 in progress -- plan 01-01 done, 01-02 next |
| Language | Rust |
| Deployment | Docker Compose (gateway + Postgres) |

## Current Position

| Field | Value |
|-------|-------|
| Phase | 01-foundation-config |
| Plan | 01-02 (next) |
| Status | Plan 01-01 complete, ready for 01-02 |

**Overall Progress:**
```
Phase  1 [=] Foundation & Config (1/2 plans)
Phase  2 [ ] MCP Protocol Layer
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
| Phases completed | 0/10 |
| Plans completed | 1/? |
| Requirements completed | 0/47 |
| Session count | 1 |

## Accumulated Context

### Key Decisions
- Rust for performance, learning, and single-binary deployment
- PostgreSQL for audit logs and config persistence
- Docker Compose for deployment (coexist with ContextForge during dev)
- Proprietary license (decide distribution later)
- stdio backend management is the key differentiator (no other gateway does this)
- rmcp 0.16 for protocol types only (not its server runtime)
- Bounded channels everywhere (no unbounded from wrapper pattern)
- JSON-RPC ID remapping from Phase 1 (architectural decision that cannot change later)

### Known Gotchas
- Rust builds require `dangerouslyDisableSandbox: true` (bwrap loopback error)
- Docker commands also need sandbox disabled
- rmcp 0.16 is pre-1.0 with 35% doc coverage -- wrap behind internal traits
- npx creates process groups -- must kill entire group, not just parent
- Pre-install npm packages globally (never use npx in production)
- SSE client disconnect is NOT cancellation per MCP spec
- Auth bypass pitfall: RBAC must filter both tools/list AND tools/call

### Blockers
- None

### TODOs
- Execute Phase 1 Plan 02 (JSON-RPC types + ID remapper)

## Session Continuity

### Last Session
- **Date:** 2026-02-22
- **What happened:** Executed 01-01-PLAN.md -- Cargo project scaffold with typed TOML config system and 8 integration tests
- **Stopped at:** Completed 01-01-PLAN.md
- **Next step:** Execute 01-02-PLAN.md (JSON-RPC 2.0 types and ID remapper)

---
*State initialized: 2026-02-22*
*Last updated: 2026-02-22*
