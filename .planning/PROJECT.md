# Sentinel Gateway

## What This Is

A Rust-based enterprise MCP (Model Context Protocol) gateway that replaces IBM ContextForge. Routes MCP tool calls from AI clients to backend MCP servers through a single governed chokepoint — providing JWT authentication, per-tool RBAC, Postgres audit logging, rate limiting, kill switches, and operational metrics. Manages both HTTP and stdio backends with crash recovery, circuit breaking, and zero-downtime config reload.

## Core Value

Every MCP tool call passes through one governed point with auth, audit, and rate limiting — no ungoverned escape hatches.

## Requirements

### Validated

- ✓ JWT authentication with HS256 token validation on every request — v1.0
- ✓ RBAC — per-tool, per-role authorization checked against token claims — v1.0
- ✓ Request routing — match tool names to HTTP backends (n8n, sqlite) and stdio backends (context7, firecrawl, exa, playwright, sequential-thinking) — v1.0
- ✓ stdio backend management — spawn, monitor, crash-detect, restart with exponential backoff, process group kill on shutdown — v1.0
- ✓ Tool discovery — aggregate `tools/list` schemas from all backends into unified catalog — v1.0
- ✓ Audit logging — structured log of every tool call to Postgres (request ID, timestamp, client, tool, backend, args, status, latency) — v1.0
- ✓ Rate limiting — per-client per-tool token bucket with retry-after semantics — v1.0
- ✓ Kill switch — disable individual tools or entire backends, hot-reloadable via SIGHUP — v1.0
- ✓ Health endpoints — `/health` (liveness), `/ready` (readiness), `/metrics` (Prometheus) — v1.0
- ✓ Circuit breaker — per-backend, open after N failures, half-open probe, auto-close — v1.0
- ✓ Schema validation — reject invalid tool arguments at gateway with descriptive JSON-RPC errors — v1.0
- ✓ Hot config reload — SIGHUP triggers atomic swap of kill switch + rate limit config — v1.0
- ✓ Docker deployment — multi-stage Dockerfile + Compose with Postgres — v1.0

### Active

#### v1.1 Deploy & Harden
- [ ] Deploy Sentinel to VPS, replacing ContextForge as the live MCP gateway
- [ ] Update Claude Code MCP config to point at Sentinel
- [ ] Verify all tools work end-to-end through Sentinel
- [ ] Harden network binding (127.0.0.1 only, iptables verified)
- [ ] n8n health monitoring with Discord alerts on failure
- [ ] Grafana dashboard for Prometheus metrics (request rates, latencies, errors, backend status)

### Out of Scope

- OAuth 2.1 (full spec) — JWT is sufficient for v1, OAuth adds complexity without value at single-user scale
- mTLS between gateway and backends — localhost/Docker network, TLS not needed internally
- OPA policy engine — simple RBAC config covers needs
- Multi-tenancy — single user on VPS, no tenant isolation needed
- Plugin system — extensibility deferred to v2
- OpenTelemetry tracing — Prometheus metrics + Postgres audit sufficient for v1
- A2A protocol — agent-to-agent is separate concern
- Caching layer — tool calls are mostly write/query operations
- Blue/green deployments — single instance, Docker restart is sufficient
- Web admin UI — config files + CLI
- Streaming HTTP transport (client-side) — stdio transport covers current use case

## Context

### Current State (v1.0 shipped)

- **Binary:** ~3,776 lines of Rust across 20 modules
- **Dependencies:** 32 crates (tokio, axum, reqwest, sqlx, prometheus, jsonschema, rmcp, etc.)
- **Tests:** 138 (33 unit + 105 integration), all passing
- **Docker:** Multi-stage build, ~50-100 MB runtime image
- **Backends:** 7 configured (2 HTTP: n8n, sqlite; 5 stdio: context7, firecrawl, exa, playwright, sequential-thinking)

### Reference Implementation
- ContextForge at `/home/lwb3/mcp-context-forge/` — still running, to be replaced when Sentinel is deployed
- IBM/Anthropic whitepaper at `/home/lwb3/Architecting-secure-enterprise-AI-agents-with-MCP.pdf`

## Constraints

- **Language**: Rust — learning goal, performance, single binary
- **Deployment**: Docker Compose — gateway + Postgres, replacing ContextForge
- **State**: PostgreSQL — audit logs, migrations embedded at compile time
- **Compatibility**: Must produce identical MCP responses to ContextForge for existing tools
- **Resources**: <100 MB RAM target, shares VPS with 14 other containers
- **License**: Proprietary (All Rights Reserved)

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust over Python | Learning + performance + single-binary deployment | ✓ Good — 3.7k LOC, fast build, tiny binary |
| PostgreSQL for state | Match ContextForge, proven, familiar | ✓ Good — sqlx + embedded migrations work well |
| Docker Compose deployment | Standard pattern, matches current infra | ✓ Good |
| Proprietary license | Build privately, decide distribution later | — Pending |
| Replace ContextForge as v1 goal | Concrete success criteria | ✓ Good — all 47 requirements met |
| Govern all 7 servers | stdio management brings ungoverned servers under gateway | ✓ Good — dual transport (HTTP + stdio) working |
| Session-level JWT auth (process exit on bad token) | stdio transport = one session per process | ✓ Good — simpler than per-request JSON-RPC errors |
| Explicit Prometheus registry (not global) | Testable metrics in isolation | ✓ Good — 4 metric unit tests pass cleanly |
| Single SharedHotConfig RwLock | Atomic swap prevents partial config state | ✓ Good — no race conditions between kill switch and rate limiter |
| SSE full-buffer accumulation | MCP backends return single-event SSE streams | ⚠️ Revisit if streaming backends added |
| Localhost-only deployment | No public exposure needed — Claude runs on same VPS | — Pending |
| Clean cutover (not parallel) | 138 tests + 47 requirements verified, rollback is trivial | — Pending |

## Current Milestone: v1.1 Deploy & Harden

**Goal:** Replace ContextForge with Sentinel on the VPS, add monitoring and network hardening.

**Target features:**
- Clean cutover from ContextForge to Sentinel
- Network hardening (127.0.0.1 binding, iptables verification)
- n8n health monitoring → Discord alerts
- Grafana dashboard for Prometheus metrics

---
*Last updated: 2026-02-22 after v1.1 milestone start*
