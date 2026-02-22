# Domain Pitfalls

**Domain:** Rust MCP Gateway (protocol gateway, process manager, enterprise security)
**Researched:** 2026-02-22
**Overall confidence:** HIGH (grounded in MCP spec, Rust ecosystem docs, and existing wrapper analysis)

## Critical Pitfalls

Mistakes that cause rewrites, security vulnerabilities, or fundamental architectural problems.

---

### Pitfall 1: MCP Spec Version Drift

**What goes wrong:** Building against the 2025-03-26 spec while the 2025-11-25 spec is already released. The newer spec adds async Tasks, enhanced OAuth 2.1 with PKCE, protocol extensions, and server discovery via well-known URLs. Claude Code or other clients may negotiate features the gateway does not understand.

**Why it happens:** The MCP spec is evolving rapidly (three major revisions in 12 months). Training data and even recent blog posts reference older versions.

**Consequences:** Clients send capabilities/requests the gateway rejects or mishandles. Worse: the gateway negotiates a protocol version it cannot fully support, causing silent failures on features like JSON-RPC batching or task handles.

**Prevention:**
- Target 2025-03-26 for v1 (matches PROJECT.md), but design the protocol layer as a version-negotiated abstraction
- Parse `protocolVersion` from `InitializeRequest` and reject unsupported versions with a clear error
- Keep a `PROTOCOL_VERSIONS_SUPPORTED` constant that is easy to extend
- Test with both 2025-03-26 and 2025-11-25 `initialize` payloads

**Detection:** Client sends `protocolVersion: "2025-11-25"` and gateway does not recognize it; batched requests silently dropped.

**Phase:** Phase 1 (protocol layer foundation) -- get version negotiation right from the start.

---

### Pitfall 2: Streamable HTTP Session Management Race Conditions

**What goes wrong:** The Streamable HTTP transport requires session ID management (`Mcp-Session-Id` header). Race conditions occur when: (a) multiple requests arrive before initialization completes, (b) session expires mid-stream, (c) client reconnects with stale session ID.

**Why it happens:** The spec says the server MAY assign a session ID at initialization, and clients MUST include it on all subsequent requests. But the initialization response might not have returned yet when the client fires off a second request. Session expiration (server returns 404) requires the client to re-initialize, but the gateway is the server here -- it must handle these transitions cleanly for its own upstream clients AND manage separate sessions with each downstream backend.

**Consequences:** Requests routed without session context. Audit logs lose session correlation. Backend state becomes inconsistent. Worst case: auth bypass if session validation is skipped during race window.

**Prevention:**
- Use a concurrent hashmap (e.g., `dashmap`) for session state, keyed by session ID
- Generate session IDs as UUIDs (not JWTs -- simpler, spec-compliant, no parsing overhead)
- Reject pre-initialization requests with 400, not silently queue them
- Implement session expiration with explicit TTL; return 404 per spec when expired
- Gateway maintains separate session tracking for upstream (Claude Code) and downstream (backends) -- these are different sessions

**Detection:** Log `Mcp-Session-Id` on every request. Alert on requests missing session ID after initialization. Monitor for 404 responses to clients.

**Phase:** Phase 2 (Streamable HTTP transport) -- this is the core transport layer concern.

---

### Pitfall 3: stdio Backend Zombie/Orphan Processes

**What goes wrong:** The gateway spawns `npx @upstash/context7-mcp`, `npx firecrawl-mcp`, etc. as child processes. When the gateway crashes, restarts, or the child hangs, zombie or orphan processes accumulate. `npx` itself spawns a shell which spawns node, creating a process tree -- killing the `npx` process does not kill the grandchild node process.

**Why it happens:** `npx` wraps the actual MCP server in a shell layer. `tokio::process::Command` kills the direct child but not the process group. On Linux, orphaned children get reparented to PID 1 (init/systemd), not to the new gateway instance. The existing wrapper analysis notes unbounded channels -- a hung child means the channel fills without bound.

**Consequences:** Resource exhaustion (zombie PIDs, leaked file descriptors, memory). Duplicate MCP servers responding to the same tool. Port conflicts if servers bind to ports. Eventually, the system runs out of PIDs or memory.

**Prevention:**
- Spawn with `kill_on_drop(true)` on the `tokio::process::Command`
- Use process groups: call `pre_exec` to `setsid()` or `setpgid()`, then kill the entire process group (`kill(-pgid, SIGTERM)`) on shutdown
- Track child PIDs in a registry; on gateway startup, check for stale PIDs from a previous run (PID file or process scan)
- Set bounded channels between stdio reader/writer and the gateway core -- apply backpressure rather than OOM
- Implement health pings: send `{"jsonrpc":"2.0","method":"ping","id":1}` periodically; if no response within timeout, kill and respawn
- Use `waitpid` (via `child.wait()`) to reap zombies; never drop the `Child` handle without waiting or killing

**Detection:** Monitor process count. Alert on child processes older than expected lifetime. Log respawn events with reason.

**Phase:** Phase 3 (stdio backend management) -- this is the entire purpose of that phase.

---

### Pitfall 4: JSON-RPC Request ID Collision Across Backends

**What goes wrong:** Claude Code sends `tools/call` with `"id": 1`. The gateway forwards this to backend A. Meanwhile, backend B (spawned as stdio child) independently sends a server-to-client request also using `"id": 1`. The gateway cannot distinguish which response belongs to which request because IDs are only unique within a single JSON-RPC connection, not globally.

**Why it happens:** JSON-RPC 2.0 says the `id` MUST be unique per connection. But the gateway multiplexes multiple backend connections onto one upstream connection. The existing Rust wrapper uses a fast ID parser (`actson`) but does not remap IDs -- it just forwards them. In a multi-backend gateway, this breaks.

**Consequences:** Response delivered to wrong tool call. Data corruption. Auth decisions applied to wrong request. Audit log records wrong tool for a given response.

**Prevention:**
- Implement ID remapping: gateway assigns its own monotonically increasing ID for each outbound request to a backend, maps the original client ID to the remapped ID, then restores the original ID when the response returns
- Use an `AtomicU64` counter for gateway-assigned IDs (fast, no contention)
- Maintain a `HashMap<RemappedId, (OriginalId, BackendId)>` per session for correlation
- Never pass client IDs through to backends unmodified
- Handle the case where a backend sends unsolicited notifications (no ID) -- these need routing metadata from context, not ID matching

**Detection:** Log both original and remapped IDs. Assert that every response has a pending request entry. Alert on unmatched responses.

**Phase:** Phase 1 (protocol layer) -- fundamental to correct routing. Must be designed before any backend communication.

---

### Pitfall 5: Auth Bypass via Tool Discovery

**What goes wrong:** The `tools/list` response aggregates tools from all backends. If RBAC filtering is applied only at `tools/call` time, a low-privilege client can see tool names and schemas for tools they cannot execute. This leaks information about capabilities (tool names, parameter schemas, descriptions). Worse: if a client guesses tool names not in their filtered list and the gateway only checks visibility (not executability), the call might succeed.

**Why it happens:** Tool discovery and tool execution are separate code paths. Developers implement RBAC on execution first ("the important one") and forget to filter the catalog. The ContextForge reference implementation may or may not filter -- it is not documented.

**Consequences:** Information disclosure (tool names reveal internal capabilities). Potential auth bypass if execution-time RBAC has a bug and discovery-time filtering was the intended defense.

**Prevention:**
- Apply RBAC filtering at BOTH `tools/list` (hide unauthorized tools) AND `tools/call` (reject unauthorized calls)
- Use the same RBAC check function for both paths -- never duplicate authorization logic
- Write tests: request `tools/list` with a restricted role, verify unauthorized tools are absent; then call an unauthorized tool directly, verify rejection
- Treat `tools/list` as a data query that requires authorization, not a public endpoint

**Detection:** Diff `tools/list` responses across roles. Any tool visible to a role that cannot execute it is a bug.

**Phase:** Phase 2 (auth/RBAC layer) -- must be designed alongside tool catalog, not bolted on later.

---

### Pitfall 6: SSE Stream Lifecycle Mismatch

**What goes wrong:** The Streamable HTTP spec says: when a POST contains JSON-RPC requests, the server MUST return either `application/json` or `text/event-stream`. If SSE, the stream SHOULD eventually include one response per request. The gateway must proxy this correctly: if a backend returns SSE, the gateway must stream it to the client, not buffer it. If the gateway closes the SSE stream before all responses are sent, the client loses data. If the client disconnects, the spec says disconnection SHOULD NOT be interpreted as cancellation -- the gateway must not kill the backend request.

**Why it happens:** Natural instinct is to cancel backend work when the client disconnects (saves resources). But the MCP spec explicitly says disconnection is not cancellation. The gateway must continue processing and be ready to redeliver if the client reconnects (resumability). Implementing this correctly requires per-stream message buffering and event ID assignment.

**Consequences:** Lost responses. Client sees partial results. Backend work completed but result never delivered. Retry storms if client keeps reconnecting and gateway keeps restarting backend work.

**Prevention:**
- Implement the SSE proxy as a true streaming pass-through: read chunks from backend, write chunks to client, do not buffer entire response
- On client disconnect: keep backend request alive, buffer results for potential redelivery (bounded buffer with TTL)
- Assign SSE event IDs per-stream (monotonic counter); support `Last-Event-ID` header for resumption
- For v1, it is acceptable to NOT implement resumability -- but document this limitation and return 405 on GET to the MCP endpoint if not supported
- Never close the SSE stream until all responses for the POST's requests have been sent

**Detection:** Monitor for SSE streams closed without all responses sent. Log client disconnects separately from cancellations.

**Phase:** Phase 2 (Streamable HTTP transport) -- deeply intertwined with session management.

---

## Moderate Pitfalls

---

### Pitfall 7: Unbounded Channel Memory Explosion

**What goes wrong:** The existing Rust wrapper uses unbounded `flume` channels between reader, workers, and writer. If a backend stalls (e.g., playwright running a long browser automation), the reader keeps accepting requests from Claude Code, queuing them without limit. Memory grows until OOM.

**Why it happens:** Unbounded channels are simpler to reason about (no backpressure, no deadlock from full channels). The wrapper is a thin pipe, not a gateway -- it did not need backpressure. A gateway does.

**Prevention:**
- Use bounded channels everywhere (`flume::bounded(N)` or `tokio::sync::mpsc::channel(N)`)
- When a channel is full, return JSON-RPC error `-32000` ("Server busy") to the client rather than queuing indefinitely
- Set channel bounds based on expected concurrency (e.g., 100 pending requests per backend)
- Monitor channel utilization as a metric

**Detection:** Track channel depth. Alert when depth exceeds 80% of bound.

**Phase:** Phase 1 (core architecture) -- channel design is foundational.

---

### Pitfall 8: Rate Limiter State Loss on Restart

**What goes wrong:** Token bucket state stored in-memory resets on gateway restart. A client that was rate-limited gets a fresh bucket after restart. If rate limit state is in Postgres, the gateway adds a DB query to every request's hot path.

**Why it happens:** Rate limiting lives in tension between performance (in-memory) and durability (persistent). The PROJECT.md mentions Postgres for state, but per-request DB queries add latency.

**Prevention:**
- Use in-memory rate limiting (e.g., `governor` crate with GCRA algorithm) as primary enforcement -- fast, no DB hit
- Persist rate limit state to Postgres asynchronously (periodic snapshots, not per-request)
- On startup, load last-known state from Postgres; accept that a restart gives a brief grace period
- For v1 single-instance, in-memory only is acceptable -- document the restart-resets-limits behavior
- If using `tower-governor`, create ONE configuration instance and share it via `Arc` -- creating multiple instances creates independent limiters (a documented gotcha)

**Detection:** Log rate limit resets. Compare pre-restart and post-restart bucket state.

**Phase:** Phase 2 (rate limiting) -- design the persistence strategy before implementation.

---

### Pitfall 9: Audit Log Argument Leakage

**What goes wrong:** Audit logs record tool call arguments for forensic analysis. But some arguments contain sensitive data: SQL queries with WHERE clauses on personal data, file paths to credentials, API keys passed as tool parameters. Logging everything creates a second copy of sensitive data in the audit table.

**Why it happens:** The whitepaper says "who, what, when, why with redaction" but implementing redaction is hard. The easy path (log everything) violates the spirit of least privilege. The hard path (redact selectively) requires knowing which fields are sensitive per tool.

**Prevention:**
- Log tool name, request ID, timestamp, client identity, and result status ALWAYS
- Log arguments at a configurable level: `full` (development), `redacted` (production), `none` (paranoid)
- Implement a deny-list of argument field names to redact: `password`, `token`, `secret`, `key`, `sql` (for queries containing PII)
- For v1, default to `redacted` mode: log argument keys but hash/truncate values over 100 characters
- Never log response bodies by default -- they may contain query results with PII

**Detection:** Grep audit logs for common secret patterns periodically. Review audit schema with security checklist.

**Phase:** Phase 2 (audit logging) -- design the redaction strategy before writing audit records.

---

### Pitfall 10: `npx` Startup Latency and Network Dependency

**What goes wrong:** `npx @upstash/context7-mcp` downloads the package on first run, adding 5-30 seconds of latency. If npm registry is unreachable, the stdio backend fails to start entirely. Even cached runs have ~2s startup overhead from Node.js + npm resolution.

**Why it happens:** The existing ungoverned servers use `npx` for convenience -- Claude Code starts them on demand. The gateway needs them running persistently, but the `npx` pattern was designed for ephemeral use.

**Prevention:**
- Pre-install all npm-based MCP servers globally (`npm install -g`) or in a local `node_modules` -- never use `npx` in production
- In TOML config, specify the actual binary path (e.g., `/usr/local/lib/node_modules/@upstash/context7-mcp/dist/index.js`) not `npx`
- Start all stdio backends at gateway startup (eager initialization), not on first request (lazy)
- Implement startup timeout: if backend does not respond to `initialize` within 10 seconds, mark unhealthy and log error
- Cache the `node_modules` in the Docker image layer during build

**Detection:** Measure backend startup time. Alert on startups exceeding threshold.

**Phase:** Phase 3 (stdio backend management) -- configuration design must account for this.

---

### Pitfall 11: Tokio Runtime Blocking in JSON Parsing

**What goes wrong:** Parsing large JSON-RPC responses (e.g., `tools/list` aggregating schemas from 7 backends, or large `resources/read` results) on the async runtime blocks the executor. `serde_json::from_slice` is synchronous CPU work. On a 4-core machine shared with 14 containers, even 50ms of blocking can cause request timeouts for other concurrent requests.

**Why it happens:** Rust's async model requires all `.await` points to yield quickly. CPU-bound JSON parsing does not yield. The existing wrapper uses a streaming JSON parser (`actson`) for ID extraction -- good -- but full response parsing still uses `serde_json`.

**Prevention:**
- Use `tokio::task::spawn_blocking` for any JSON parsing of payloads larger than ~64KB
- For the common case (small tool call responses), synchronous parsing on the async runtime is fine
- Profile with `tokio-console` to detect blocking tasks
- Consider `simd-json` for large payloads (2-3x faster than `serde_json` on x86_64)
- The gateway should minimize full deserialization -- for routing, only parse the `method` and `id` fields (like the existing fast path), pass the rest through as raw bytes

**Detection:** Enable tokio's `tracing` integration. Watch for tasks that run >10ms without yielding.

**Phase:** Phase 1 (core architecture) -- decide the parsing strategy early.

---

### Pitfall 12: Backend Health Check False Positives

**What goes wrong:** Gateway pings backends with `ping` method. Backend responds to ping but is actually broken (e.g., n8n container is up but its workflow engine is deadlocked, or sqlite backend responds to ping but the database file is locked). The gateway marks it healthy and routes requests, which then fail.

**Why it happens:** Health checks test reachability, not functionality. A proper health check for an MCP backend would need to execute a lightweight tool call, not just `ping`.

**Prevention:**
- Two-tier health checks: (1) `ping` for basic liveness (fast, frequent), (2) a canary tool call for readiness (slow, less frequent)
- For HTTP backends: also check HTTP status code, not just response body
- For stdio backends: check that the process is still running AND responding to messages (pipe may be open but process wedged)
- Implement circuit breaker pattern: after N consecutive failures, stop routing to that backend; periodically probe to detect recovery
- `tools/list` is a good readiness check -- it exercises the backend's core functionality without side effects

**Detection:** Compare health check results with actual tool call success rates. Divergence indicates false positives.

**Phase:** Phase 2 (health checks) -- design alongside routing.

---

## Minor Pitfalls

---

### Pitfall 13: TOML Config Hot Reload Partial Application

**What goes wrong:** Gateway watches TOML config file for changes (SIGHUP or file watch). A partial config update (e.g., adding a new backend but not updating RBAC) creates an inconsistent state where the backend is registered but no role can access it, or worse, a permissive default allows all roles.

**Prevention:**
- Validate config as a complete unit before applying -- reject partial/invalid configs and keep running with the old config
- Default deny: if a tool has no explicit RBAC rule, deny access (never default allow)
- Log config reload events with a diff of what changed

**Phase:** Phase 4 (operational concerns).

---

### Pitfall 14: Docker Network Isolation Assumptions

**What goes wrong:** The gateway needs to reach both HTTP backends on Docker networks (mcpnet) AND spawn stdio processes that access the internet (e.g., context7 fetches docs, firecrawl crawls URLs). Running the gateway in a Docker container with restricted networking breaks stdio backends that need internet access.

**Prevention:**
- Gateway container needs `--network=host` or a Docker network with internet egress
- Alternatively, stdio backends run outside Docker (on host) while gateway runs in Docker -- requires careful port/socket mapping
- Document the network topology decision explicitly in architecture docs
- Test each stdio backend's network requirements before containerizing

**Phase:** Phase 4 (Docker deployment) -- but must be considered during Phase 3 (stdio management) design.

---

### Pitfall 15: Notification Handling (Fire-and-Forget Messages)

**What goes wrong:** JSON-RPC notifications have no `id` field and expect no response. If the gateway tries to correlate them (waiting for a response that never comes), it leaks memory in the pending-request map. If the gateway ignores them, it misses important protocol messages like `notifications/initialized`, `notifications/cancelled`, or `notifications/progress`.

**Prevention:**
- Detect notifications by the absence of an `id` field (per JSON-RPC 2.0 spec)
- Route notifications to the correct backend but do not add to pending-request map
- Handle protocol-level notifications (`initialized`, `cancelled`) in the gateway itself, not forwarded
- Forward tool-level notifications to the appropriate backend based on context

**Phase:** Phase 1 (protocol layer) -- core message routing concern.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Phase 1: Protocol & Core | ID collision across backends (Pitfall 4) | Implement ID remapping from day one |
| Phase 1: Protocol & Core | Notification mishandling (Pitfall 15) | Classify messages by type before routing |
| Phase 1: Protocol & Core | Blocking JSON parsing (Pitfall 11) | Use fast-path extraction, defer full parsing |
| Phase 1: Protocol & Core | Unbounded channels (Pitfall 7) | Bounded channels with backpressure |
| Phase 2: Transport & Auth | Session race conditions (Pitfall 2) | Concurrent hashmap, reject pre-init requests |
| Phase 2: Transport & Auth | SSE lifecycle mismatch (Pitfall 6) | Stream pass-through, no premature close |
| Phase 2: Transport & Auth | Auth bypass via discovery (Pitfall 5) | Same RBAC function for list and call |
| Phase 2: Transport & Auth | Rate limiter restart (Pitfall 8) | In-memory primary, async persistence |
| Phase 2: Transport & Auth | Audit argument leakage (Pitfall 9) | Redaction by default |
| Phase 3: stdio Management | Zombie processes (Pitfall 3) | Process groups, kill_on_drop, health pings |
| Phase 3: stdio Management | npx latency (Pitfall 10) | Pre-install, eager startup, no npx |
| Phase 4: Deployment | Config hot reload races (Pitfall 13) | Atomic config validation before swap |
| Phase 4: Deployment | Docker network isolation (Pitfall 14) | Test network access per backend type |
| Ongoing | Spec version drift (Pitfall 1) | Version negotiation, track spec releases |
| Ongoing | Health check false positives (Pitfall 12) | Two-tier checks: liveness + readiness |

## Sources

- [MCP Specification 2025-03-26: Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) -- Streamable HTTP requirements, session management, SSE rules (HIGH confidence)
- [MCP Specification 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25) -- Latest spec version with Tasks, OAuth 2.1, extensions (HIGH confidence)
- [MCP 2025-11-25 Spec Update (WorkOS)](https://workos.com/blog/mcp-2025-11-25-spec-update) -- Async tasks, PKCE mandatory, protocol extensions (MEDIUM confidence)
- [Why MCP Deprecated SSE (fka.dev)](https://blog.fka.dev/blog/2025-06-06-why-mcp-deprecated-sse-and-go-with-streamable-http/) -- SSE deprecation rationale (MEDIUM confidence)
- [tokio::process::Child docs](https://docs.rs/tokio/latest/tokio/process/struct.Child.html) -- kill_on_drop, stdio deadlock prevention (HIGH confidence)
- [tokio ChildStdin termination detection issue](https://github.com/tokio-rs/tokio/issues/2174) -- Linux-specific stdio child process bug (HIGH confidence)
- [tower-governor rate limiting](https://github.com/benwis/tower-governor) -- Configuration sharing gotcha (HIGH confidence)
- [MCP Security Risks (Red Hat)](https://www.redhat.com/en/blog/model-context-protocol-mcp-understanding-security-risks-and-controls) -- Enterprise security concerns (MEDIUM confidence)
- [MCP Security (Zenity)](https://zenity.io/blog/security/securing-the-model-context-protocol-mcp) -- Prompt injection, tool permission risks (MEDIUM confidence)
- [Rust Wrapper Analysis](/home/lwb3/sentinel-gateway/docs/RUST-WRAPPER-ANALYSIS.md) -- Existing patterns, unbounded channels, fast ID parser (HIGH confidence, local)
- [MCP Topology](/home/lwb3/sentinel-gateway/docs/MCP-TOPOLOGY.md) -- Current architecture, ungoverned server risks (HIGH confidence, local)
