# Project Research Summary

**Project:** Sentinel Gateway (Rust MCP Gateway)
**Domain:** Protocol gateway — MCP routing, authentication, and governance
**Researched:** 2026-02-22
**Confidence:** HIGH

## Executive Summary

Sentinel Gateway is a Rust replacement for IBM ContextForge: a single-binary MCP gateway that federates multiple backend MCP servers behind a unified auth, RBAC, rate limiting, and audit layer. The domain is well-understood — 17+ open-source gateways and 21 commercial gateways have been catalogued, with IBM/Anthropic publishing a formal whitepaper defining the 7 minimum responsibilities of any enterprise MCP gateway. The recommended approach is tokio + axum + sqlx + rmcp, reusing all crate choices already proven in the existing Rust wrapper, with Tower middleware composition as the core architectural pattern. The gateway bridges two upstream transports (stdio from Claude Code, Streamable HTTP for remote clients) to two backend types (HTTP MCP servers like n8n/sqlite, and stdio MCP servers like context7/firecrawl/exa).

The biggest differentiator and the hardest feature is stdio backend management: no other gateway governs stdio MCP servers. ContextForge routes only to HTTP backends; Sentinel governs all MCP traffic under one chokepoint. This earns a second-class differentiator status: single binary deployment (~10-20 MB, no Python runtime, no pip, no gunicorn) versus ContextForge's 1.2 GB total footprint (512 MB Python gateway + 512 MB Postgres + 192 MB Redis). These are not incidental wins — they are the primary reasons to build this.

The key risks are protocol-level: JSON-RPC request ID collision across backends must be solved from day one (Phase 1), stdio child process lifecycle management is genuinely hard (zombie/orphan processes, process groups, npx startup latency), and SSE stream lifecycle rules in the Streamable HTTP spec are counterintuitive (client disconnect is NOT cancellation). The mitigation for all of these is well-understood and documented; none require research breakthroughs, only careful implementation. Build bottom-up: foundation first, HTTP backends before stdio backends, middleware after routing.

## Key Findings

### Recommended Stack

The stack is almost entirely determined by the existing Rust wrapper (890 lines, already running in production as the ContextForge wrapper). All critical crates are already proven: tokio 1.47 LTS (async runtime), reqwest 0.13 (HTTP client), flume 0.12 (MPMC channels), arc-swap (lock-free config), mimalloc (allocator), tracing (structured logging), clap 4.5 (CLI), bytes (zero-copy buffers). New additions for the gateway layer are axum 0.8 (HTTP server), tower 0.5 (middleware), sqlx 0.8 (Postgres — async, compile-time query checking), jsonwebtoken 10 (JWT HS256), governor 0.10 (GCRA rate limiting), and rmcp 0.16 (official MCP Rust SDK for protocol types).

The only medium-confidence choice is rmcp 0.16 — it is pre-1.0 with 35% doc coverage and 22 releases since March 2025. The mitigation is explicit: use rmcp for protocol types only, not its server runtime; wrap its types behind internal traits; pin the exact version (`"=0.16.0"`). The gateway is a custom router, not a standard MCP server, so rmcp's runtime is irrelevant.

**Core technologies:**
- tokio 1.47 LTS: async runtime — industry standard, LTS until Sept 2026, all ecosystem crates depend on it
- axum 0.8: HTTP server (upstream Streamable HTTP transport) — Tower-native, stable API since Jan 2025
- tower 0.5 + tower-http 0.6: middleware composition — one middleware stack works for both stdio and HTTP transports
- reqwest 0.13 + rustls 0.23: HTTP client to backends — already proven in wrapper, pure-Rust TLS for simple Docker builds
- sqlx 0.8: Postgres client — compile-time query checking, async, no ORM overhead
- jsonwebtoken 10: JWT HS256 validation — de facto Rust JWT library, aws_lc_rs backend
- governor 0.10: GCRA token bucket rate limiting — per-key (client+tool) limiting, 64 bits per state, thread-safe
- rmcp 0.16: MCP protocol types — official SDK, use for types/spec compliance only
- flume 0.12 (bounded): MPMC channels — switch unbounded (wrapper) to bounded (gateway) for backpressure

**Deferred explicitly:** Redis (not needed for single-instance rate limiting), OpenTelemetry (tracing + Postgres audit logs sufficient), OPA (TOML RBAC covers all v1 needs), Actix Web (own runtime model, avoid), tonic/gRPC (wrong protocol).

### Expected Features

The IBM/Anthropic whitepaper defines 7 minimum gateway responsibilities; all 14 table-stakes features map directly to these. There is no ambiguity about what a gateway must do. The feature research surveyed the actual ContextForge implementation as a live reference, giving HIGH confidence in the feature list.

**Must have (table stakes):**
- JWT authentication — every request proves identity via HS256 token
- RBAC per-tool per-role — token claims map to roles, roles map to tool permissions via TOML
- Request routing — tool name patterns to HTTP or stdio backends
- Tool discovery (catalog aggregation) — merge `tools/list` from all backends into one unified catalog
- Rate limiting — in-memory token bucket per (client, tool); no Redis needed for single-node
- Audit logging — who/what/when/args (redacted)/status/latency to Postgres
- Health checks — `/health` + `/ready` + periodic backend pings
- Kill switch — disable tool or backend without restart; hot-reloadable
- JSON-RPC 2.0 compliance — request correlation, error codes, notification handling
- Dual transport — stdio upstream (Claude Code) bridging to HTTP and stdio backends
- SSE streaming — proxy backend SSE events without buffering to client
- Connection management — timeouts, retries, exponential backoff with jitter
- Graceful shutdown — drain in-flight, kill children cleanly, flush audit logs
- TOML configuration — all config externalized, no hardcoded values

**Should have (differentiators):**
- stdio backend management — governs context7, firecrawl, exa, playwright, sequential-thinking; no other gateway does this
- Single binary deployment — Rust binary replaces Python+pip+gunicorn+Redis stack
- Hot config reload — SIGHUP or file-watch, zero downtime config changes
- Circuit breaker per backend — Open/Half-Open/Closed state machine, prevents cascading failures
- Input validation — validate tool arguments against cached JSON schemas before forwarding
- Prometheus `/metrics` — request counts, latencies, error rates, backend health
- Request ID correlation — UUID per request, propagated through logs and headers
- CLI management tool — `sentinel-cli status/tools/kill/token` as admin interface

**Defer to v2+:**
- OAuth 2.1 with PKCE — JWT is sufficient for single-user VPS, OAuth is enterprise scope
- Web admin UI — TOML config + CLI is faster for single operator, no XSS surface
- Plugin system — hard-code the right features, extensibility is v2
- Multi-tenancy — single user, zero benefit for current scale
- OPA policy engine — TOML allow/deny rules cover all v1 RBAC needs
- mTLS between gateway and backends — all localhost/Docker, no benefit
- OpenTelemetry / distributed tracing — structured logs + Prometheus covers observability needs
- Caching layer — tool calls are mostly write/unique-read, staleness risk not worth it

### Architecture Approach

The architecture is layered bottom-up with clear component boundaries and no circular dependencies. Claude Code connects via stdio (newline-delimited JSON-RPC); the Transport Layer parses this into typed `McpRequest` structs; the Protocol Layer enforces the MCP lifecycle state machine (Created → Initializing → Operational → Closed); requests flow through a Tower middleware chain (Auth → RBAC → Rate Limit → Kill Switch → Audit in that order); the Router maps tool names to the correct backend connector (HTTP via reqwest or stdio via child process); responses flow back up through audit completion logging. The key architectural insight is that Tower layers compose across both transports — the same middleware stack protects requests arriving via stdio and HTTP upstream, preventing auth bypass at transport seams.

**Major components:**
1. Transport Layer (stdio + Streamable HTTP) — parses protocol-specific wire format into unified `McpRequest`; build stdio first (simpler), add HTTP upstream later
2. Protocol Layer (MCP lifecycle state machine) — enforces spec rules, handles capability merging, manages session IDs
3. Middleware Chain (Tower layers) — Auth, RBAC, Rate Limit, Kill Switch, Audit as composable, independently testable layers
4. Router + Tool Registry — maps tool names to backends; owns the merged catalog from all `tools/list` responses
5. HTTP Backend Connector — reqwest client with pooling, SSE passthrough, retry/backoff; reuses wrapper patterns directly
6. Stdio Backend Manager — spawns/supervises child processes (context7, firecrawl, etc.), multiplexes JSON-RPC over stdin/stdout with oneshot-channel request correlation; the hardest component
7. Config System (TOML + ArcSwap) — hot-reloadable, typed, validated as a unit before applying
8. Postgres Client (sqlx) — async audit writes via bounded channel to dedicated writer task; never blocks the data plane

### Critical Pitfalls

1. **JSON-RPC request ID collision across backends** — Gateway must remap client IDs to gateway-assigned monotonic IDs before forwarding; restore original ID on response. Use `AtomicU64` counter. Never pass client IDs through unmodified. Design this in Phase 1 before any backend communication.

2. **stdio child process zombies/orphans** — `npx` creates a process group (shell → node), killing the parent leaves orphaned grandchildren. Mitigation: `kill_on_drop(true)`, use `setsid()`/process groups, kill the entire group on shutdown, run health pings and respawn on failure. Pre-install npm packages globally — never use `npx` in production (adds 2-30s startup latency, network dependency).

3. **Auth bypass via tool discovery** — Apply RBAC filtering at both `tools/list` (hide unauthorized tools) AND `tools/call` (reject unauthorized calls). Use the same RBAC check function for both code paths. Unauthorized tools must be invisible, not just unexecutable.

4. **SSE stream lifecycle per spec** — Client disconnect is NOT cancellation (explicit MCP spec rule). Gateway must continue processing and be ready to redeliver. For v1, acceptable to not implement full resumability, but must not kill backend work on disconnect. True streaming passthrough — never buffer the entire SSE body.

5. **Unbounded channels from wrapper pattern** — The existing wrapper uses unbounded `flume` channels. The gateway must use bounded channels everywhere with backpressure. When a backend stalls, the correct behavior is to apply backpressure to the client, not accumulate unbounded memory toward OOM.

## Implications for Roadmap

Based on combined research, the architecture's build-order dependency graph maps directly to phases. There is no ambiguity: foundation must precede protocol must precede routing must precede middleware. Stdio backends are definitively last (hardest, most novel, no existing wrapper pattern to reuse). HTTP upstream transport is optional for v1 stdio use case.

### Phase 1: Foundation and Core Protocol

**Rationale:** Every other component depends on config loading, logging, and JSON-RPC types. The protocol layer (MCP state machine, ID remapping, notification classification) must exist before any backend communication. These are critical architectural decisions that cannot be changed later without rewrites. ID remapping in particular is a Phase 1 requirement — it must be designed before any backend sends a single request.

**Delivers:** A compiling binary that reads stdin, parses MCP initialize/tools-list/tools-call/ping messages, responds to initialize and ping directly, and stubs out routing. Validates the overall structure.

**Addresses:** TOML configuration, JSON-RPC 2.0 compliance, graceful shutdown (signal handler foundations)

**Avoids:** ID collision (Pitfall 4 — must design remapping now), notification mishandling (Pitfall 15), unbounded channels (Pitfall 7 — set bounded channel sizes from the start), blocking JSON parsing (Pitfall 11 — fast-path extraction strategy)

### Phase 2: HTTP Backend Routing and Governance

**Rationale:** HTTP backends (n8n, sqlite) are the simpler connector type — stateless, known pattern from the wrapper, already running. Get a working gateway that routes real requests to real backends before tackling the hard stuff. The middleware chain wraps the router, so routing must work before middleware can be tested end-to-end. Auth and RBAC must be designed alongside tool catalog (not bolted on later) due to the discovery bypass pitfall.

**Delivers:** A fully functional drop-in replacement for ContextForge's core HTTP routing capability. Routes `tools/call` to n8n and sqlite via JWT auth and RBAC. Audit logs to Postgres. Rate limits in-memory. Kill switch per tool/backend.

**Uses:** axum 0.8 (HTTP upstream transport), sqlx 0.8 (Postgres audit), governor 0.10 (rate limiting), jsonwebtoken 10 (JWT), tower middleware stack, reqwest (HTTP backends)

**Implements:** Transport Layer (HTTP upstream), Middleware Chain (all 5 layers), HTTP Backend Connector, Health Monitor, Postgres Client

**Avoids:** Auth bypass via discovery (Pitfall 5 — RBAC at both list and call), session race conditions (Pitfall 2 — DashMap for session state), SSE lifecycle (Pitfall 6 — streaming passthrough), audit leakage (Pitfall 9 — redaction by default), rate limiter restart (Pitfall 8 — in-memory primary, async persistence)

### Phase 3: stdio Backend Management

**Rationale:** This is the single biggest differentiator and the hardest component. It must come after Phase 2 because: (a) the middleware chain needs to be solid before adding a new backend type, (b) the tool catalog merge logic is simpler to develop against HTTP backends first, (c) the request correlation patterns are already proven for HTTP and can be adapted. This phase delivers Sentinel's unique value: governance over context7, firecrawl, exa, playwright, and sequential-thinking.

**Delivers:** All 7 current MCP servers (2 HTTP + 5 stdio) governed under one gateway. Unified tool catalog from all backends. Supervisor task per stdio backend with crash recovery.

**Implements:** Stdio Backend Manager, per-backend supervisor tasks, oneshot-channel request correlation, process group management

**Avoids:** Zombie/orphan processes (Pitfall 3 — process groups, kill_on_drop, health pings), npx startup latency (Pitfall 10 — pre-install globally, eager init, no npx), health check false positives (Pitfall 12 — two-tier liveness + readiness)

### Phase 4: Operational Polish and Deployment

**Rationale:** Once the gateway is functionally complete, harden the operational story: hot config reload, Prometheus metrics, CLI tooling, Docker Compose, and deployment validation. These features have no functional dependencies on each other and can be built in any order within this phase.

**Delivers:** Production-ready deployment. Zero-downtime config changes. Prometheus metrics for monitoring. CLI for emergency tool kill. Docker Compose replacing ContextForge's compose stack.

**Implements:** Hot config reload (SIGHUP + ArcSwap), Prometheus `/metrics` endpoint, `sentinel-cli` subcommands, input validation against tool schemas, request ID correlation headers

**Avoids:** Config hot reload partial application (Pitfall 13 — validate as unit before swap), Docker network isolation (Pitfall 14 — test each stdio backend's network requirements), spec version drift (Pitfall 1 — version negotiation constants)

### Phase Ordering Rationale

- ID remapping and bounded channels are Phase 1 decisions because changing them later requires touching every component
- HTTP backends before stdio backends because HTTP is stateless and the pattern is already proven in the wrapper; success here validates routing without process management complexity
- Middleware after single-backend routing because middleware wraps the router — you need a working router to test auth/RBAC end-to-end
- Stdio backends in Phase 3 because they are the hardest novel work; a solid Phase 2 foundation means bugs are isolatable to the stdio layer
- Operational polish last because it adds no capability, only reliability; the gateway should be functionally correct first

### Research Flags

Phases needing deeper research during planning:
- **Phase 3 (stdio Backend Management):** rmcp 0.16 docs are 35% complete; actual stdio child process lifecycle for npx-wrapped Node.js servers needs real-world testing. Recommend a spike: spawn `npx @upstash/context7-mcp` manually via `tokio::process`, get `initialize` round-trip working before committing to the full Stdio Backend Manager design.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Foundation):** TOML config + tokio + tracing are completely documented, no unknowns
- **Phase 2 (HTTP Routing + Governance):** axum + Tower middleware is heavily documented; all patterns have official examples and blog posts; HTTP backend routing mirrors the existing wrapper closely
- **Phase 4 (Operational Polish):** ArcSwap hot reload, prometheus-client, clap subcommands are all well-documented

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All critical crates already proven in the wrapper (890 lines, in production). New additions (axum, sqlx, governor) are mature, well-documented, and have abundant examples. rmcp is the only risk — mitigated by using it for types only. |
| Features | HIGH | Based on IBM/Anthropic whitepaper (authoritative), ContextForge live reference implementation, and survey of 17+ open-source gateways. Feature expectations are clear and well-validated. |
| Architecture | HIGH | MCP spec 2025-03-26 is the authoritative source. Tower middleware composition is well-documented. Existing wrapper provides a baseline for stdio and HTTP patterns. Only stdio backend management is novel — no prior art in the codebase. |
| Pitfalls | HIGH | Most pitfalls are grounded in the MCP spec itself (session management, SSE rules, notification handling) or in the existing wrapper's known limitations (unbounded channels). The zombie process pitfall is well-documented for tokio::process. |

**Overall confidence:** HIGH

### Gaps to Address

- **rmcp 0.16 integration depth:** The crate is pre-1.0 with sparse docs. The plan to use it for types only is correct, but the exact API surface (which types to use, which to replace) needs hands-on validation during Phase 1. Mitigation: wrap all rmcp types behind internal traits on first use.

- **stdio backend process group behavior:** The precise behavior of killing `npx`-spawned Node.js servers on Linux (process group vs individual PID, signal propagation) needs a quick spike before Phase 3 design is finalized. This is a 1-2 hour investigation, not a blocker.

- **Streamable HTTP session resumption scope:** The MCP spec allows but does not require SSE resumability. Phase 2 should document whether the gateway will support `Last-Event-ID` resumption or return 405. This is a design decision, not a research gap, but it must be made before implementing the SSE proxy.

- **Database schema for audit logs:** The PITFALLS research recommends `full/redacted/none` argument logging modes and a deny-list of sensitive field names. The exact Postgres schema (argument storage format, indexing strategy, retention policy) needs a decision before Phase 2 begins. Straightforward SQL design but not yet specified.

## Sources

### Primary (HIGH confidence)
- IBM/Anthropic whitepaper "Architecting Secure Enterprise AI Agents with MCP" (Oct 2025) — 7 minimum gateway responsibilities, RBAC patterns, audit requirements
- MCP Specification 2025-03-26 (official) — transport specs, Streamable HTTP, SSE rules, lifecycle state machine
- MCP Specification 2025-11-25 (official) — async Tasks, OAuth 2.1, protocol extensions, version negotiation
- `/home/lwb3/sentinel-gateway/docs/RUST-WRAPPER-ANALYSIS.md` — existing crate choices, fast ID parser, channel patterns, build system
- `/home/lwb3/mcp-context-forge/` (live deployment) — ContextForge reference implementation, actual tool catalog, backend topology
- `/home/lwb3/sentinel-gateway/docs/MCP-TOPOLOGY.md` — current governed vs ungoverned server map

### Secondary (MEDIUM confidence)
- [rmcp GitHub (modelcontextprotocol/rust-sdk)](https://github.com/modelcontextprotocol/rust-sdk) — official Rust SDK, v0.16.0
- [Axum 0.8.0 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) — API stability, Tower integration
- [Tower middleware for auth/logging in Axum](https://oneuptime.com/blog/post/2026-01-25-tower-middleware-auth-logging-axum-rust/view) — middleware patterns
- [governor rate limiting](https://github.com/boinkor-net/governor) — GCRA algorithm, keyed rate limiters
- [Awesome MCP Gateways](https://github.com/e2b-dev/awesome-mcp-gateways) — 17 open-source + 21 commercial gateways surveyed
- [Why MCP Deprecated SSE (fka.dev)](https://blog.fka.dev/blog/2025-06-06-why-mcp-deprecated-sse-and-go-with-streamable-http/) — SSE deprecation rationale and Streamable HTTP motivation

### Tertiary (MEDIUM/LOW confidence)
- [MCP Security Risks (Red Hat)](https://www.redhat.com/en/blog/model-context-protocol-mcp-understanding-security-risks-and-controls) — enterprise security concerns
- [MintMCP vs IBM ContextForge comparison](https://www.mintmcp.com/blog/mintmcp-vs-ibm-contextforge-comparison) — competitive landscape
- [tokio::process zombie issue #2174](https://github.com/tokio-rs/tokio/issues/2174) — Linux-specific stdio child process behavior

---
*Research completed: 2026-02-22*
*Ready for roadmap: yes*
