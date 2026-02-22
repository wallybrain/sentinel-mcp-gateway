# Feature Landscape

**Domain:** MCP Gateway (Model Context Protocol routing, auth, and governance)
**Researched:** 2026-02-22
**Overall confidence:** HIGH (based on whitepaper, ContextForge reference, and 17+ open-source gateways surveyed)

## Table Stakes

Features users expect from any MCP gateway. Missing any of these and it is not a gateway -- it is a proxy at best.

| # | Feature | Why Expected | Complexity | Notes |
|---|---------|--------------|------------|-------|
| 1 | **JWT Authentication** | Every request must prove identity. The whitepaper lists identity brokering as minimum responsibility #1. Every gateway in the ecosystem implements auth. | Medium | HS256 validation, exp/iss/aud/jti checks. ContextForge uses this exact scheme. |
| 2 | **RBAC (per-tool, per-role)** | Without authorization, auth is pointless. Whitepaper minimum #5. Even single-user deployments need role separation for future growth. | Medium | Token claims map to roles, roles map to tool permissions. TOML config, not OPA. |
| 3 | **Request Routing** | The core gateway function: match tool names to backend servers. Without routing, there is no gateway. Whitepaper minimum #3. | Medium | Route table mapping tool name patterns to HTTP backends or stdio processes. |
| 4 | **Tool Discovery (catalog aggregation)** | Clients call `tools/list` and expect one unified catalog. Whitepaper minimum #2. ContextForge's virtual server concept does exactly this. | Medium | Merge `tools/list` responses from all backends, deduplicate, serve as one catalog. |
| 5 | **Rate Limiting** | Prevent runaway agents from hammering backends. Whitepaper minimum #4. Token bucket per-client, per-tool. | Medium | In-memory token bucket with configurable rates per tool. No Redis needed for single-node. |
| 6 | **Audit Logging** | Who called what, when, with what arguments, what happened. Whitepaper minimum #6. Non-negotiable for enterprise and useful for debugging at any scale. | Medium | Structured JSON logs to Postgres. Fields: timestamp, client, tool, args (redacted), status, latency. |
| 7 | **Health Checks** | `/health` and `/ready` endpoints plus periodic backend pings. Whitepaper minimum #3 (availability monitoring). Docker and orchestrators depend on this. | Low | Liveness (gateway up) and readiness (backends reachable). Backend ping on configurable interval. |
| 8 | **Kill Switch** | Disable individual tools or entire backends without restarting. Whitepaper minimum #7. Emergency response capability. | Low | Config flag per tool/backend. Check on every request. Hot-reloadable via SIGHUP or file watch. |
| 9 | **JSON-RPC 2.0 Compliance** | MCP IS JSON-RPC 2.0. Non-compliance means non-functional. Request/response correlation, error codes, batch support. | Medium | Must handle: id correlation, method dispatch, error objects, notification (no id). |
| 10 | **Dual Transport: stdio upstream + Streamable HTTP downstream** | Claude Code connects via stdio. HTTP backends speak Streamable HTTP. Gateway must bridge both. This is THE core MCP topology. | High | stdio: newline-delimited JSON-RPC on stdin/stdout. HTTP: POST with JSON body, SSE response stream. |
| 11 | **SSE Streaming** | Backends return `text/event-stream` for long-running tool calls. Gateway must proxy these without buffering. Dropping SSE = breaking streaming tools. | Medium | Proxy SSE events from backend to client without buffering entire response. |
| 12 | **Connection Management** | Keep-alive, timeouts, retries with exponential backoff and jitter. Without this, transient failures cascade. Whitepaper lists this under scalability. | Medium | Per-backend configurable timeout, max retries, backoff factor. Connection pooling via reqwest. |
| 13 | **Graceful Shutdown** | Handle SIGTERM: drain in-flight requests, close stdio children, flush audit logs, then exit. Ungraceful shutdown = lost audit data and orphaned processes. | Low | Signal handler, shutdown timeout, ordered teardown. |
| 14 | **TOML Configuration** | Backends, roles, rate limits, kill switches all in config. No hardcoded values. Whitepaper calls for externalized configuration. | Low | Single `sentinel.toml` with sections for auth, backends, roles, limits. |

## Differentiators

Features that set Sentinel apart from ContextForge and other MCP gateways. Not expected but valued.

| # | Feature | Value Proposition | Complexity | Notes |
|---|---------|-------------------|------------|-------|
| 1 | **stdio Backend Management** | No other gateway governs stdio MCP servers (context7, firecrawl, exa, playwright, sequential-thinking). ContextForge only routes to HTTP backends. This is the single biggest differentiator -- bringing ALL MCP traffic under one governed chokepoint. | High | Spawn child processes, manage lifecycle (restart on crash), multiplex JSON-RPC over stdin/stdout, aggregate into unified catalog. The hardest feature in the gateway. |
| 2 | **Single Binary Deployment** | Rust compiles to one static binary. No Python runtime, no pip, no virtualenv, no gunicorn. ContextForge needs Python 3.11+, FastAPI, Gunicorn, and 40+ pip packages. `cargo build --release` produces a ~10-20 MB binary. | Free (Rust) | Massively reduces deployment complexity and attack surface. Ship one file. |
| 3 | **Minimal Resource Footprint** | Target: <50 MB RAM vs ContextForge's 512 MB limit (plus 512 MB Postgres, 192 MB Redis = 1.2 GB total). Sentinel targets <100 MB total (gateway + embedded state). Eliminates Redis entirely. | Medium | Rust async runtime (tokio) is inherently efficient. No GC pauses. No Redis needed for single-node rate limiting. |
| 4 | **Hot Config Reload** | Change TOML config, send SIGHUP (or file-watch detects change), gateway picks up new roles/limits/kill-switches without restart. Zero downtime config changes. | Medium | Watch config file or handle SIGHUP. Re-parse TOML, swap Arc'd config atomically. Existing connections unaffected. |
| 5 | **Circuit Breaker per Backend** | When a backend fails N times, stop sending requests for a cooldown period. Prevents cascading failures. Standard in API gateways (Kong, Envoy) but absent from most MCP gateways. | Medium | Per-backend state machine: Closed -> Open (after N failures) -> Half-Open (probe) -> Closed. |
| 6 | **Input Validation** | Validate tool call arguments against the tool's JSON schema before forwarding. Reject malformed requests at the gateway. Whitepaper lists this under security foundations. | Medium | Cache tool schemas from `tools/list`, validate `arguments` on `tools/call`. Return JSON-RPC error for invalid input. |
| 7 | **Prometheus Metrics Endpoint** | `/metrics` with request counts, latencies, error rates, backend health, rate limit hits. Standard observability that ContextForge buries in its Postgres DB. | Low | Use prometheus-client crate. Counters and histograms for key operations. |
| 8 | **Request ID Correlation** | Generate a unique request ID for each tool call, propagate through audit logs, backend requests, and response headers. Enables end-to-end tracing without OpenTelemetry. | Low | UUID per request, attached to all log entries and passed as header to backends. |
| 9 | **CLI Management Tool** | `sentinel-cli status`, `sentinel-cli tools`, `sentinel-cli kill <tool>`, `sentinel-cli token generate`. No web UI needed -- config files + CLI cover all management. | Medium | Separate binary or subcommand that talks to gateway's admin API (or reads config directly). |

## Anti-Features

Features to explicitly NOT build in v1. These add complexity without value at current scale, or are better solved elsewhere.

| # | Anti-Feature | Why Avoid | What to Do Instead |
|---|--------------|-----------|-------------------|
| 1 | **OAuth 2.1 (full spec)** | OAuth adds PKCE, token refresh, authorization server, client registration -- massive complexity for a single-user VPS. JWT is sufficient and proven (ContextForge uses it). The whitepaper says "OAuth per MCP spec" but that is enterprise scope. | JWT with HS256. Add OAuth in v2 if multi-user demand materializes. |
| 2 | **Web Admin UI** | ContextForge has one; we never use it. Config files + CLI are faster for a single operator. UI adds a frontend framework, build pipeline, session management, and XSS surface. | TOML config + CLI tool. The config file IS the admin interface. |
| 3 | **Plugin System** | ContextForge has 40+ plugins; we use zero. Plugins need a runtime, API surface, versioning, security sandboxing. Build the right features into core instead. | Hard-code the features that matter. Extensibility is a v2 concern. |
| 4 | **Multi-tenancy** | Single user on a VPS. Tenant isolation needs separate configs, separate rate limits, separate audit trails, separate catalogs. Enormous complexity for zero current users. | Single-tenant with clean boundaries so multi-tenancy CAN be added later. |
| 5 | **OPA Policy Engine** | Policy-as-code (Rego) is powerful but overkill. Simple TOML allow/deny rules cover "can role X call tool Y?" without learning a policy language or running a sidecar. | TOML-based role-to-tool mappings. If rules get complex enough to need OPA, add it. |
| 6 | **mTLS Between Gateway and Backends** | All traffic is localhost or Docker network. TLS adds certificate management, rotation, and debugging pain for zero security benefit on a local network. | Plain HTTP internally. Add mTLS in v2 if gateway and backends are ever on separate hosts. |
| 7 | **A2A Protocol Support** | Agent-to-Agent is a separate concern from tool governance. The whitepaper mentions it but as a future pattern. No current A2A clients exist in this infrastructure. | Focus on MCP. Revisit A2A when agent orchestration is a real need. |
| 8 | **Caching Layer** | Tool calls are mostly write operations (execute workflow, run query) or unique reads (query with different params). Caching adds staleness risk and invalidation complexity. | No caching. If specific tools benefit from caching later, add per-tool cache config. |
| 9 | **OpenTelemetry / Distributed Tracing** | Structured logs to Postgres + Prometheus metrics + request ID correlation cover observability needs. OTel adds a collector, exporter config, and span management overhead. | Structured audit logs + `/metrics` + request IDs. Add OTel in v2 if log correlation proves insufficient. |
| 10 | **LLM Provider Proxying** | ContextForge can proxy OpenAI-compatible LLM calls. That is a different product (AI gateway, not MCP gateway). Mixing concerns dilutes focus. | Sentinel is an MCP gateway. LLM routing belongs in a separate layer. |
| 11 | **REST-to-MCP Conversion** | ContextForge converts arbitrary REST APIs to MCP tools. Useful but complex (schema inference, error mapping, pagination). Backend MCP servers already handle this. | Backends expose MCP natively. If a REST API needs MCP wrapping, write a thin MCP server for it. |
| 12 | **Blue/Green or Canary Deployments** | Single instance, Docker restart is fine. Deployment strategies add routing complexity, health check sophistication, and state management. | `docker compose up -d --build` with graceful shutdown. Rolling deploys in v2. |
| 13 | **Model/Schema Versioning** | Tool schemas come from backends. Gateway should not version-manage them -- that is the backend's responsibility. | Pass through backend schemas as-is. Pin backend versions in Docker Compose. |

## Feature Dependencies

```
                    JWT Authentication
                          |
                          v
                    RBAC (per-tool)
                          |
                    +-----+-----+
                    |           |
                    v           v
             Request Routing   Kill Switch
                    |
          +---------+---------+
          |                   |
          v                   v
   HTTP Backend          stdio Backend
   Routing               Management
          |                   |
          +--------+----------+
                   |
                   v
          Tool Discovery
          (catalog merge)
                   |
          +--------+--------+
          |        |        |
          v        v        v
    Rate        Audit     Circuit
    Limiting    Logging   Breaker
                   |
                   v
            Input Validation
            (needs tool schemas)
```

Key dependency chains:
- **Auth before RBAC** -- cannot check permissions without identity
- **Routing before Discovery** -- cannot aggregate catalogs without reaching backends
- **Discovery before Validation** -- cannot validate args without tool schemas
- **stdio Management is independent of HTTP routing** -- can be built in parallel but both feed into Discovery
- **Health Checks independent** -- can exist before any routing works

## MVP Recommendation

**Phase 1 -- Foundation (get requests flowing):**
1. TOML configuration system
2. JWT authentication
3. RBAC (role-to-tool mapping)
4. HTTP backend routing (n8n, sqlite -- match ContextForge behavior)
5. Tool discovery (aggregate `tools/list` from HTTP backends)
6. Health check endpoints

**Phase 2 -- Governance (make it production-worthy):**
1. Audit logging to Postgres
2. Rate limiting (in-memory token bucket)
3. Kill switch (per-tool, per-backend)
4. SSE streaming passthrough
5. Connection management (timeouts, retries, backoff)
6. Graceful shutdown

**Phase 3 -- stdio (the differentiator):**
1. stdio backend management (spawn, lifecycle, restart)
2. stdio JSON-RPC multiplexing
3. Unified catalog (HTTP + stdio backends merged)
4. Circuit breaker per backend

**Phase 4 -- Polish (operational excellence):**
1. Hot config reload (SIGHUP)
2. Input validation against tool schemas
3. Prometheus metrics endpoint
4. Request ID correlation
5. CLI management tool

**Rationale:** Phase 1 produces a drop-in replacement for ContextForge's core routing. Phase 2 adds the governance that makes it enterprise-grade. Phase 3 is the unique value -- no other gateway governs stdio servers. Phase 4 is operational polish that makes it pleasant to run.

**Defer:** Everything in the anti-features list. If any become needed, they are v2 scope.

## Sources

- IBM/Anthropic whitepaper "Architecting Secure Enterprise AI Agents with MCP" (Oct 2025), pp. 14-18
- [IBM ContextForge documentation](https://ibm.github.io/mcp-context-forge/)
- [ContextForge GitHub](https://github.com/IBM/mcp-context-forge)
- [Awesome MCP Gateways](https://github.com/e2b-dev/awesome-mcp-gateways) -- 17 open-source + 21 commercial gateways catalogued
- [MCP Architecture Overview](https://modelcontextprotocol.io/docs/learn/architecture)
- [Best MCP Gateways for Platform Engineering 2026](https://www.mintmcp.com/blog/mcp-gateways-platform-engineering-teams)
- [MintMCP vs IBM ContextForge](https://www.mintmcp.com/blog/mintmcp-vs-ibm-contextforge-comparison)
- [Why MCP Needs a Gateway](https://bytebridge.medium.com/why-mcp-needs-a-gateway-turning-model-context-protocol-integrations-into-production-grade-agent-88f80f390f49)
- [MCP Timeout and Retry Strategies](https://octopus.com/blog/mcp-timeout-retry)
- [Avoid stdio! MCP Servers In Enterprise Should Be Remote](https://blog.christianposta.com/mcp-should-be-remote/)
- ContextForge deployment at `/home/lwb3/mcp-context-forge/` (live reference implementation)
- MCP Topology analysis at `/home/lwb3/sentinel-gateway/docs/MCP-TOPOLOGY.md`
