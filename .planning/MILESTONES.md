# Milestones

## v1.0 Sentinel Gateway MVP (Shipped: 2026-02-22)

**Phases completed:** 9 phases, 20 plans | **Tests:** 138 | **LOC:** 3,776 Rust

**Delivered:** Single-binary Rust MCP gateway governing 7 backend servers (2 HTTP + 5 stdio) with JWT auth, RBAC, audit logging, rate limiting, kill switches, Prometheus metrics, schema validation, and hot config reload.

**Key accomplishments:**
1. Single-binary MCP gateway — Rust binary loads TOML config, speaks MCP protocol, aggregates 7 backend tool catalogs
2. Dual transport routing — HTTP backends (n8n, sqlite) + stdio backends (context7, firecrawl, exa, playwright, sequential-thinking)
3. JWT auth + per-tool RBAC — every request authenticated, tools filtered per role
4. Postgres audit trail — async logging with UUID request IDs, latency, caller identity
5. Rate limiting + kill switch — token bucket per-client per-tool, instant tool/backend disable, hot-reloadable via SIGHUP
6. Production reliability — health endpoints, circuit breakers, graceful shutdown, stdio crash recovery, Prometheus metrics, schema validation

---

