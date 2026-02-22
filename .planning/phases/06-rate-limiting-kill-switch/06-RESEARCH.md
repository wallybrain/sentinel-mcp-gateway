# Phase 6: Rate Limiting & Kill Switch - Research

**Researched:** 2026-02-22
**Domain:** In-memory rate limiting, feature flags/kill switches, JSON-RPC error semantics
**Confidence:** HIGH

## Summary

Phase 6 adds two defensive features to the gateway dispatch loop: per-client-per-tool rate limiting using an in-memory token bucket, and a kill switch that can disable individual tools or entire backends via config. Both features intercept requests in the `run_dispatch` function in `gateway.rs`, before the request reaches the backend.

The existing codebase already defines `RateLimitConfig` (with `default_rpm` and `per_tool` HashMap) and `KillSwitchConfig` (with `disabled_tools` and `disabled_backends` vectors) in `config/types.rs`, and `sentinel.toml` already has sections for both. The implementation work is: (1) build the rate limiter state struct, (2) add kill switch checks to the dispatch loop, (3) add rate limit checks to the dispatch loop, (4) return proper JSON-RPC errors with retry-after semantics.

**Primary recommendation:** Hand-roll a simple token bucket (~60 lines) using `DashMap<(client, tool), TokenBucket>` rather than pulling in the `governor` crate. The requirement is straightforward (RPM per client-per-tool), the codebase already avoids unnecessary dependencies, and the implementation is trivial.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| RATE-01 | Gateway enforces per-client, per-tool rate limits using in-memory token bucket | Token bucket struct with DashMap keyed by (client_subject, tool_name); checked in dispatch loop before backend call |
| RATE-02 | Rate limit configuration is defined per-tool in TOML config with sensible defaults | Already defined in config/types.rs: `RateLimitConfig` with `default_rpm: 1000` and `per_tool: HashMap<String, u32>` |
| RATE-03 | Rate-limited requests receive JSON-RPC error with retry-after semantics | JSON-RPC error code -32004 with `data.retryAfter` field (seconds until next token available) |
| KILL-01 | Gateway can disable individual tools via config (requests return JSON-RPC error) | Check `kill_switch.disabled_tools` in dispatch loop before RBAC check; return -32005 error |
| KILL-02 | Gateway can disable entire backends via config (all tools on that backend return error) | Check `kill_switch.disabled_backends` via catalog.route() lookup; return -32005 error |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::collections::HashMap | stdlib | Token bucket storage (wrapped in Mutex or use DashMap) | No external dependency needed for simple case |
| std::time::Instant | stdlib | Token refill timing | Sub-microsecond precision, no allocation |
| DashMap | 6.x | Concurrent per-key bucket storage | Already a transitive dep via other crates; lock-free reads |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled token bucket | `governor` 0.10.x (3.4M downloads/month) | Governor is production-grade but adds ~6 transitive deps for what is 60 lines of code. Use governor if requirements grow to include burst, sliding window, or distributed limiting. |
| DashMap | `std::sync::Mutex<HashMap>` | Mutex is simpler (no new dep) but holds lock during check+update. DashMap gives per-shard locking. For this gateway's concurrency level (single stdio transport), Mutex is fine. |

**Decision:** Use `std::sync::Mutex<HashMap<(String, String), TokenBucket>>` to avoid adding any new dependency. The gateway processes one request at a time on the stdio transport, so contention is zero. If HTTP transport is added later, swap to DashMap.

**Installation:**
```bash
# No new dependencies needed
```

## Architecture Patterns

### New Module Structure
```
src/
├── ratelimit/
│   ├── mod.rs           # TokenBucket struct + RateLimiter
│   └── (single file is fine, no submodules needed)
├── gateway.rs           # Add kill switch + rate limit checks
└── config/types.rs      # Already has RateLimitConfig + KillSwitchConfig
```

### Pattern 1: Token Bucket Implementation
**What:** A fixed-window token bucket that refills tokens every minute
**When to use:** Every `tools/call` request
**Example:**
```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct TokenBucket {
    tokens: u32,
    max_tokens: u32,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u32) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            last_refill: Instant::now(),
        }
    }

    /// Try to consume a token. Returns Ok(()) or Err(seconds_until_refill).
    fn try_consume(&mut self) -> Result<(), f64> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        // Refill if a minute has passed
        if elapsed >= 60.0 {
            self.tokens = self.max_tokens;
            self.last_refill = now;
        }

        if self.tokens > 0 {
            self.tokens -= 1;
            Ok(())
        } else {
            let retry_after = 60.0 - elapsed;
            Err(retry_after.max(1.0))
        }
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<(String, String), TokenBucket>>,
    default_rpm: u32,
    per_tool: HashMap<String, u32>,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            default_rpm: config.default_rpm,
            per_tool: config.per_tool.clone(),
        }
    }

    /// Check rate limit for a client+tool pair.
    /// Returns Ok(()) or Err(retry_after_seconds).
    pub fn check(&self, client: &str, tool: &str) -> Result<(), f64> {
        let rpm = self.per_tool.get(tool).copied().unwrap_or(self.default_rpm);
        let key = (client.to_string(), tool.to_string());
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(key)
            .or_insert_with(|| TokenBucket::new(rpm));
        bucket.try_consume()
    }
}
```

### Pattern 2: Kill Switch Check in Dispatch Loop
**What:** Early return before RBAC/backend routing if tool or backend is disabled
**When to use:** Every `tools/call` and `tools/list` request
**Example:**
```rust
// In run_dispatch, tools/call branch, BEFORE rbac check:
if kill_switch.disabled_tools.contains(&tool_name.to_string()) {
    let resp = JsonRpcResponse::error(
        id,
        KILL_SWITCH_ERROR,
        format!("Tool is disabled: {tool_name}"),
    );
    send_response(&tx, &resp).await;
    // audit with status "killed"
    continue;
}

// Check backend-level kill switch
if let Some(backend_name) = catalog.route(tool_name) {
    if kill_switch.disabled_backends.contains(&backend_name.to_string()) {
        let resp = JsonRpcResponse::error(
            id,
            KILL_SWITCH_ERROR,
            format!("Backend is disabled: {backend_name}"),
        );
        send_response(&tx, &resp).await;
        // audit with status "killed"
        continue;
    }
}
```

### Pattern 3: Dispatch Loop Order of Operations
**What:** The correct ordering of checks in the dispatch loop
**Why:** Kill switch should be first (cheapest, no state), then rate limit, then RBAC, then backend call
```
tools/call request arrives
  1. Extract tool name
  2. Kill switch check (tool disabled?) ──> -32005 error
  3. Kill switch check (backend disabled?) ──> -32005 error
  4. Rate limit check (client+tool) ──> -32004 error with retryAfter
  5. RBAC check (existing) ──> -32003 error
  6. Route to backend (existing)
```

### Pattern 4: tools/list Filtering for Kill Switch
**What:** Disabled tools should be excluded from `tools/list` responses
**Why:** Clients should not see tools they cannot call
```rust
// In tools/list handler, add kill switch filter alongside RBAC filter:
let tools: Vec<_> = catalog
    .all_tools()
    .into_iter()
    .filter(|tool| {
        !kill_switch.disabled_tools.contains(&tool.name.to_string())
    })
    .filter(|tool| {
        if let Some(backend) = catalog.route(&tool.name) {
            !kill_switch.disabled_backends.contains(&backend.to_string())
        } else {
            true
        }
    })
    .filter(|tool| {
        is_tool_allowed(&caller.role, &tool.name, Permission::Read, rbac_config)
    })
    .collect();
```

### Anti-Patterns to Avoid
- **Background drip task for token refill:** Not needed. Refill on access (lazy refill) is simpler and uses zero resources when idle.
- **Unbounded HashMap growth:** If clients disconnect and reconnect with different subjects, stale buckets accumulate. Add periodic cleanup or cap the map size.
- **Rate limiting notifications:** Only rate-limit `tools/call` requests. `initialize`, `notifications/*`, `ping` should never be rate-limited.
- **Blocking on rate limit check:** The Mutex lock is held only for the duration of a HashMap lookup + token decrement (~nanoseconds). Never hold it across an await point.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Distributed rate limiting | Redis-backed limiter | N/A (out of scope per REQUIREMENTS.md) | Explicitly excluded: "In-memory rate limiting sufficient for single-node" |
| Sliding window algorithm | Complex windowed counter | Simple fixed-window token bucket | RPM granularity is sufficient; sliding window adds complexity for marginal accuracy improvement |

**Key insight:** The gateway is single-node, single-transport (stdio). A simple Mutex<HashMap> with lazy refill is the right level of complexity. Do not over-engineer.

## Common Pitfalls

### Pitfall 1: Rate Limit Key Design
**What goes wrong:** Using only tool name (not client) as the rate limit key means one client's usage affects all clients.
**Why it happens:** The requirement says "per-client, per-tool" but it's easy to overlook the per-client part.
**How to avoid:** Key is `(client_subject, tool_name)` tuple. The `CallerIdentity.subject` from JWT is the client identifier.
**Warning signs:** Rate limit tests pass with a single client but fail when two clients are tested concurrently.

### Pitfall 2: Stale Bucket Accumulation
**What goes wrong:** HashMap grows unbounded as unique (client, tool) pairs accumulate.
**Why it happens:** Clients connect, make calls, disconnect. Their buckets remain in memory.
**How to avoid:** Periodically prune buckets that haven't been accessed in >5 minutes. Or accept the leak for v1 (each bucket is ~24 bytes; 10K unique pairs = 240KB).
**Warning signs:** Memory growth over time in long-running gateway.

### Pitfall 3: Kill Switch Not Filtering tools/list
**What goes wrong:** Client sees a tool in `tools/list` but gets an error when calling it.
**Why it happens:** Kill switch only checked in `tools/call` path, not `tools/list`.
**How to avoid:** Filter disabled tools from `tools/list` response, same as RBAC filtering.
**Warning signs:** Client reports "tool exists but cannot be called."

### Pitfall 4: Audit Entry for Rate-Limited/Killed Requests
**What goes wrong:** Rate-limited or killed requests are not audited, creating gaps in the audit trail.
**Why it happens:** Early return before the existing audit code at the end of `tools/call`.
**How to avoid:** Emit audit entries with `status = "rate_limited"` or `status = "killed"` at the point of rejection, same pattern as RBAC denied entries.
**Warning signs:** Missing entries in audit log for rejected requests.

### Pitfall 5: Error Code Collisions
**What goes wrong:** Using an existing error code for rate limiting or kill switch.
**Why it happens:** Not checking the existing error code assignments.
**How to avoid:** The codebase uses: -32700 (parse), -32601 (method not found), -32602 (invalid params), -32603 (internal), -32001 (auth), -32002 (not initialized), -32003 (authz). Use -32004 for rate limit, -32005 for kill switch.
**Warning signs:** Client cannot distinguish rate limit errors from auth errors.

## Code Examples

### JSON-RPC Rate Limit Error Response
```json
{
    "jsonrpc": "2.0",
    "id": 42,
    "error": {
        "code": -32004,
        "message": "Rate limit exceeded for tool: execute_workflow",
        "data": {
            "retryAfter": 45
        }
    }
}
```

### JSON-RPC Kill Switch Error Response
```json
{
    "jsonrpc": "2.0",
    "id": 42,
    "error": {
        "code": -32005,
        "message": "Tool is disabled: execute_workflow"
    }
}
```

### Updated run_dispatch Signature
```rust
pub async fn run_dispatch(
    mut rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<String>,
    catalog: &ToolCatalog,
    backends: &HashMap<String, HttpBackend>,
    id_remapper: &IdRemapper,
    caller: Option<CallerIdentity>,
    rbac_config: &RbacConfig,
    audit_tx: Option<mpsc::Sender<AuditEntry>>,
    rate_limiter: &RateLimiter,           // NEW
    kill_switch: &KillSwitchConfig,       // NEW
) -> anyhow::Result<()> {
```

### Test Pattern: Rate Limiter Unit Test
```rust
#[test]
fn rate_limiter_allows_within_limit() {
    let config = RateLimitConfig {
        default_rpm: 5,
        per_tool: HashMap::new(),
    };
    let limiter = RateLimiter::new(&config);

    for _ in 0..5 {
        assert!(limiter.check("client1", "some_tool").is_ok());
    }
    // 6th call should fail
    assert!(limiter.check("client1", "some_tool").is_err());
    // Different client should still work
    assert!(limiter.check("client2", "some_tool").is_ok());
}

#[test]
fn rate_limiter_per_tool_override() {
    let mut per_tool = HashMap::new();
    per_tool.insert("expensive_tool".to_string(), 2);

    let config = RateLimitConfig {
        default_rpm: 100,
        per_tool,
    };
    let limiter = RateLimiter::new(&config);

    assert!(limiter.check("client1", "expensive_tool").is_ok());
    assert!(limiter.check("client1", "expensive_tool").is_ok());
    assert!(limiter.check("client1", "expensive_tool").is_err());
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Redis-backed rate limiting | In-memory (single-node sufficient) | Project decision | No Redis dependency, simpler deployment |
| Middleware-based rate limiting (tower) | Inline dispatch loop check | N/A (this is not an HTTP server) | Gateway uses stdio transport, not tower/axum stack |
| `governor` crate for GCRA | Hand-rolled token bucket | Project-specific | Fewer dependencies, simpler for RPM use case |

## Open Questions

1. **Bucket cleanup strategy**
   - What we know: Stale buckets from disconnected clients accumulate
   - What's unclear: How many unique clients in practice? Is 240KB/10K pairs acceptable?
   - Recommendation: Accept the leak for v1. Add cleanup in Phase 9 (hot reload) if needed. Each bucket is ~24 bytes.

2. **Kill switch audit status string**
   - What we know: Existing statuses are "success", "error", "denied"
   - What's unclear: Should killed requests use "killed" or "disabled"?
   - Recommendation: Use "killed" for kill switch, "rate_limited" for rate limiting. Distinct from "denied" (RBAC).

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/config/types.rs` -- existing RateLimitConfig and KillSwitchConfig structs
- Codebase analysis: `src/gateway.rs` -- existing dispatch loop with RBAC pattern to follow
- Codebase analysis: `sentinel.toml` -- existing rate_limits and kill_switch config sections
- Codebase analysis: `src/protocol/jsonrpc.rs` -- existing error code constants and JsonRpcError with data field

### Secondary (MEDIUM confidence)
- [Governor crate](https://lib.rs/crates/governor) -- v0.10.4, 3.4M downloads/month, GCRA-based rate limiting
- [MCP Error Codes](https://www.mcpevals.io/blog/mcp-error-codes) -- JSON-RPC error conventions for MCP
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification) -- Error object structure with data field

### Tertiary (LOW confidence)
- [Rust rate limiting blog post](https://oneuptime.com/blog/post/2026-01-07-rust-rate-limiting/view) -- general patterns, not project-specific

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, uses existing config types
- Architecture: HIGH -- follows established dispatch loop pattern from RBAC (Phase 4)
- Pitfalls: HIGH -- derived from codebase analysis, well-understood problem domain

**Research date:** 2026-02-22
**Valid until:** 2026-04-22 (stable domain, 60-day validity)
