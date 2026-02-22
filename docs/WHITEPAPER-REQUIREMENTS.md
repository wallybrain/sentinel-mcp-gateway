# Whitepaper Requirements

Gateway requirements extracted from "Architecting Secure Enterprise AI Agents with MCP" (IBM/Anthropic, October 2025). Mapped to implementation priority for the Rust gateway.

## Source

PDF: `/home/lwb3/Architecting-secure-enterprise-AI-agents-with-MCP.pdf`
Pages 14-18 define the MCP Gateway Pattern and its minimum responsibilities.

## Minimum Gateway Responsibilities (PDF p.14)

The whitepaper lists these as the **minimum** for an enterprise MCP gateway:

| # | Responsibility | PDF Description | Priority |
|---|---------------|-----------------|----------|
| 1 | **Identity & scope brokering** | Authenticate clients, broker OAuth tokens/scopes, enforce per-tool permissions | v1 Core |
| 2 | **Catalog / registry** | Service discovery, tool enumeration with schemas | v1 Core |
| 3 | **Routing & health checks** | Route requests to backends, monitor availability | v1 Core |
| 4 | **Rate limits & quotas** | Per-tenant, per-tool limits; "try later" semantics | v1 Core |
| 5 | **Policy enforcement** | Policy-as-code (OPA); tool allow/deny, environment gating | v1 Simple |
| 6 | **Audit & metrics** | Structured trails: who, what, when, why; success/latency/error metrics | v1 Core |
| 7 | **Emergency kill switches** | Disable individual tools or entire backends | v1 Core |

## Extended Gateway Capabilities (PDF p.14-15)

Beyond the minimum, the whitepaper describes:

| Capability | Description | Priority |
|------------|-------------|----------|
| **Centralized control** | Single place for auth, routing, rate limiting, quotas, discovery | v1 Core |
| **Security boundary** | TLS termination, mTLS to backends, prompt security | v2 |
| **Policy & guardrails** | OPA for tool allow/deny, environment gating, approval workflows, sensitive data handling | v2 |
| **Multitenancy** | Per-tenant isolation for configs, keys, logs, metrics, limits, catalogs; dev/stage/prod routes | v2 |
| **Governance & audit** | Standardized logging, request correlation, audit trails across all servers | v1 Core |
| **Reliability & scale** | HA, autoscaling, circuit breaking, retries with idempotency, backpressure, traffic shaping | v2 |
| **Compatibility layer** | Feature detection, capability negotiation, schema normalization, version pinning | v2 |
| **Plugins** | Pre/post hooks for observability, PII filtering, XSS/profanity filters, auth extensions | v2 |

## Security Foundations (PDF p.15)

| Requirement | Description | Priority |
|-------------|-------------|----------|
| **OAuth per MCP spec** | Proper auth flows, token refresh | v1 (JWT first, OAuth later) |
| **Least privilege** | Read-only default, per-tool/per-parameter authorization | v1 Core |
| **Input validation** | Strict schemas, types, ranges; reject invalid immediately | v1 Core |
| **Output sanitization** | Prevent injection into downstream systems | v2 |
| **Secrets in managers** | Never inline credentials | v1 Core |
| **TLS everywhere** | Enforce TLS for all transport | v2 (localhost first) |
| **Sandboxing** | Plugins in sandboxed environments | v2 |

## Tooling Discipline (PDF p.15)

| Principle | Description | Priority |
|-----------|-------------|----------|
| **Clear descriptions** | Purpose, constraints, side effects, usage guidance | v1 Core |
| **Stable versioned interfaces** | Tool schema versioning | v2 |
| **Bounded capabilities** | Small, focused tools over kitchen-sink | N/A (backend concern) |
| **Read-only deployments** | Dynamic tool enablement by tenant/role/env | v2 |
| **Stateless execution** | Default stateless; externalize state with TTLs | v1 Core |
| **Async patterns** | Handles, status tools, callbacks for long-running ops | v2 |
| **Idempotency** | Client keys, compensating/rollback actions | v2 |

## Scalability & Resiliency (PDF p.16)

| Requirement | Description | Priority |
|-------------|-------------|----------|
| **Horizontal scale** | Concurrent, short-lived requests; idempotent retries | v2 |
| **Rate limiting** | Per-tenant, per-tool; backpressure | v1 Core |
| **Health endpoints** | `/health`, `/ready`; circuit breakers | v1 Core |
| **Caching** | Read-heavy ops with TTL; batch requests | v2 |
| **Versioning** | Server, tool schemas, side-effect contracts | v2 |
| **Connection management** | HTTP keep-alive, timeouts, retry with jitter | v1 Core |

## Governance (PDF p.16)

| Requirement | Description | Priority |
|-------------|-------------|----------|
| **Structured audit trails** | Who, what, when, why with redaction | v1 Core |
| **Centralized guardrails** | Policy-as-code, consistent across environments | v2 |
| **Curated catalogs** | Approved servers/tools with ownership, versions, risk levels | v2 |
| **Data classification** | Locality, retention, minimization, redaction | v2 |
| **SBOMs** | Vulnerability scans, container signing, dependency policies | v2 |

## Reference Architecture Requirements (PDF p.19)

### Non-Functional Requirements

| Category | Key Requirements | Priority |
|----------|-----------------|----------|
| **Architecture** | MCP Gateway for routing + policy; REST<->MCP conversion; virtual servers | v1 Core |
| **Build-time security** | RBAC, workspaces, access logging | v2 |
| **Run-time security** | Agent identity, OAuth, delegation, granular auth; data encryption; audit logs | v1 Core |
| **Observability** | Full telemetry (MELT), distributed tracing, conversational logging | v2 |
| **Governance** | Safety guardrails, drift detection, governed catalogs, supply-chain risk | v2 |
| **Resilience** | Self-healing, failover, graceful degradation; cost controls | v2 |
| **Deployment** | Air-gapped to hyperscaler; bring-your-own auth/secrets; portability | v1 (Docker) |

### Functional Requirements

| Category | Key Requirements | Priority |
|----------|-----------------|----------|
| **Memory & state** | Short/long-term memory, session persistence, vector DB integration | v2 |
| **Planning & execution** | Task decomposition, human-in-the-loop, safe tool orchestration | N/A (agent concern) |
| **Interoperability** | MCP for tools/resources/prompts, A2A patterns, OpenAI-compatible APIs | v1 (MCP only) |
| **Human-agent collaboration** | Approvals, escalations, debug mode, trace inspection | v2 |
| **Performance & evaluation** | Behavior logging, scoring, champion-challenger, CI/CD gates | v2 |

## v1 Scope Summary

Based on the whitepaper's minimum gateway responsibilities, the Rust gateway v1 should implement:

### Must Have (v1)
- JWT authentication with token validation
- RBAC (role-based tool access)
- Request routing to HTTP backends
- Tool discovery and schema aggregation
- Per-client, per-tool rate limiting
- Structured audit logging (to file, SQLite, or embedded DB)
- Health check endpoints (`/health`, `/ready`)
- Kill switch (disable tools/backends via config reload)
- Connection management (keep-alive, timeouts, retries)
- Both stdio and Streamable HTTP transport

### Should Have (v1.x)
- Simple policy rules (TOML/YAML allow/deny per tool per role)
- stdio backend management (spawn/manage child processes)
- Metrics endpoint (Prometheus-compatible)
- Graceful shutdown and signal handling
- Hot config reload (SIGHUP or watch file)

### Defer to v2
- OAuth 2.1 (full spec, not just JWT)
- mTLS between gateway and backends
- OPA policy engine integration
- Multi-tenancy
- Plugin system
- OpenTelemetry tracing
- A2A protocol support
- Caching layer
- Blue/green and canary deployments

## Key Whitepaper Quotes

> "Choose the language that best matches your operational model and integration profile. Optimize for maintainability, observability, and SLOs — not theoretical speed." (p.13)

> "Use an enterprise MCP Gateway when you need centralized security, control, and scale across many servers and tenants. The gateway becomes the single, policy-enforced ingress for agent access to organizational capabilities." (p.14)

> "Sandboxing should be treated as a baseline control, not an optional feature." (p.6)

> "MCP servers succeed in the enterprise when they are treated as durable products: narrowly scoped, strongly governed, observable, and easy to evolve." (p.18)

> "Example implementation: mcp-contextforge." (p.14)
