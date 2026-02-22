# Sentinel Gateway

## What This Is

A Rust-based enterprise MCP (Model Context Protocol) gateway that replaces IBM ContextForge. It routes MCP tool calls from AI clients to backend MCP servers through a single governed chokepoint — providing centralized authentication, authorization, audit logging, and rate limiting. Designed to scale from solo developer to enterprise deployment.

## Core Value

Every MCP tool call passes through one governed point with auth, audit, and rate limiting — no ungoverned escape hatches.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] JWT authentication with HS256 token validation on every request
- [ ] RBAC — per-tool, per-role authorization checked against token claims
- [ ] Request routing — match tool names to HTTP backend servers (n8n, sqlite)
- [ ] stdio backend management — spawn and manage child processes (context7, firecrawl, exa, playwright, sequential-thinking)
- [ ] Tool discovery — aggregate `tools/list` schemas from all backends into one catalog
- [ ] Audit logging — structured log of every tool call to Postgres (who, what, when, arguments, result status)
- [ ] Rate limiting — per-client, per-tool token bucket
- [ ] Health check endpoints — `/health`, `/ready` plus periodic backend pings
- [ ] Kill switch — disable individual tools or entire backends via config
- [ ] MCP protocol — JSON-RPC 2.0 over stdio (upstream) and Streamable HTTP (upstream + downstream)
- [ ] SSE streaming — handle text/event-stream responses from backends
- [ ] Config system — TOML config for backends, roles, rate limits, kill switches
- [ ] Docker Compose deployment — gateway + Postgres, network config
- [ ] Connection management — keep-alive, timeouts, retries with backoff

### Out of Scope

- OAuth 2.1 (full spec) — JWT is sufficient for v1, OAuth adds complexity without value at single-user scale
- mTLS between gateway and backends — localhost/Docker network, TLS not needed internally
- OPA policy engine — simple RBAC config covers v1 needs
- Multi-tenancy — single user on VPS, no tenant isolation needed yet
- Plugin system — build core features first, extensibility in v2
- OpenTelemetry tracing — structured logging to Postgres is sufficient for v1
- A2A protocol — agent-to-agent is a separate concern
- Caching layer — tool calls are mostly write/query operations, caching adds complexity
- Blue/green deployments — single instance, Docker restart is fine
- Web admin UI — config files + CLI for v1

## Context

### Current Infrastructure
- Ubuntu 24.04.4 LTS, 4 cores, 16 GB RAM, 193 GB disk (29% used)
- 14 Docker containers running (all healthy)
- IBM ContextForge (Python/FastAPI) governs 2 of 7 MCP servers via 5 containers (~430 MB RAM)
- 5 MCP servers (context7, firecrawl, exa, playwright, sequential-thinking) are ungoverned — direct stdio from Claude Code

### Reference Implementation
- ContextForge at `/home/lwb3/mcp-context-forge/` — working reference for behavior to match
- Rust stdio wrapper at `mcp-context-forge/tools_rust/wrapper/` — 890 lines, reusable patterns for reqwest, tokio, JSON-RPC, SSE
- IBM/Anthropic whitepaper at `/home/lwb3/Architecting-secure-enterprise-AI-agents-with-MCP.pdf` — enterprise requirements spec

### MCP Protocol
- JSON-RPC 2.0 over stdio (newline-delimited) or Streamable HTTP (POST with JSON or SSE response)
- Protocol version: 2025-03-26
- Key methods: `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`, `ping`
- Gateway must handle: request ID correlation, SSE streaming, stdio multiplexing, capability merging

### Documentation
- Full infrastructure docs in `docs/` directory (pushed to GitHub)
- Whitepaper requirements mapped to v1/v2 in `docs/WHITEPAPER-REQUIREMENTS.md`

## Constraints

- **Language**: Rust — learning goal, performance goal, and product differentiator (single binary)
- **Deployment**: Docker Compose — gateway container + Postgres, must coexist with ContextForge during development
- **State**: PostgreSQL — audit logs, config persistence, rate limit state
- **Compatibility**: Must produce identical MCP responses to ContextForge for the 19 existing tools
- **Server resources**: 4 cores, 16 GB RAM shared with 14 other containers — Rust binary should use <100 MB
- **License**: Proprietary (All Rights Reserved) — potential commercial or open-source release later
- **Pace**: No deadline — built incrementally across sessions, ContextForge runs until Sentinel is ready

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust over Python | Learning + performance + single-binary deployment | -- Pending |
| PostgreSQL for state | Match ContextForge, proven at scale, familiar | -- Pending |
| Docker Compose deployment | Standard pattern, easy to ship, matches current infra | -- Pending |
| Proprietary license | Build privately, decide distribution model later | -- Pending |
| Replace ContextForge as v1 goal | Concrete success criteria — swap on VPS, everything works | -- Pending |
| Govern all 7 servers | stdio management brings ungoverned servers under gateway | -- Pending |

---
*Last updated: 2026-02-22 after initialization*
