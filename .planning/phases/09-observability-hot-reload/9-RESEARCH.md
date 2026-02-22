# Phase 9: Observability & Hot Reload - Research

**Researched:** 2026-02-22
**Domain:** Prometheus metrics, JSON Schema validation, SIGHUP config reload (Rust/Tokio)
**Confidence:** HIGH

## Summary

Phase 9 adds three distinct capabilities to the Sentinel Gateway: (1) a Prometheus-compatible `/metrics` endpoint exposing request counts, latency histograms, error rates, backend health, and rate limit hits; (2) JSON Schema validation of tool call arguments against cached schemas from `tools/list` before forwarding to backends; and (3) hot config reload via SIGHUP signal for zero-downtime kill switch and rate limit changes.

The Rust ecosystem has mature, stable crates for all three areas. The `prometheus` crate (0.14.0) is the standard for Prometheus exposition with `TextEncoder` and labeled metric types. The `jsonschema` crate (0.42.1) provides compiled validators that can be cached per-tool for fast argument validation. SIGHUP handling is already half-implemented -- `tokio::signal::unix::SignalKind::hangup()` uses the same API as the existing SIGTERM/SIGINT handlers in `main.rs`.

**Primary recommendation:** Use the `prometheus` crate directly (not axum-prometheus middleware, since metrics are gateway-level not HTTP-level), `jsonschema` for schema validation, and `Arc<RwLock<>>` for hot-reloadable config sections (kill_switch, rate_limits).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| OBS-01 | Gateway exposes `/metrics` endpoint with Prometheus-compatible metrics | `prometheus` 0.14.0 TextEncoder + axum GET handler on existing health server |
| OBS-02 | Metrics include: request count, latency histogram, error rate, backend health, rate limit hits per tool | CounterVec (labels: tool, status), HistogramVec (labels: tool), GaugeVec (labels: backend), CounterVec (labels: tool) |
| OBS-03 | Gateway validates tool call arguments against cached JSON schemas from `tools/list` | `jsonschema` 0.42.1 validator_for() compiled once per tool from `Tool.input_schema` |
| OBS-04 | Invalid arguments rejected at gateway with descriptive JSON-RPC error before reaching backend | jsonschema iter_errors() provides path + message for INVALID_PARAMS (-32602) response |
| CONFIG-03 | Gateway supports hot config reload via SIGHUP signal or file watch | tokio::signal::unix::SignalKind::hangup() + re-parse sentinel.toml + swap Arc<RwLock<>> |
| KILL-03 | Kill switch changes take effect via hot config reload without restart | KillSwitchConfig behind Arc<RwLock<>>, swapped atomically on SIGHUP |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| prometheus | 0.14.0 | Prometheus metric types and text encoder | De facto Rust Prometheus client, 37M+ downloads, stable API since 0.13 |
| jsonschema | 0.42.1 | JSON Schema validation of tool arguments | Fastest Rust JSON Schema validator, supports Draft 4-7 + 2019-09 + 2020-12 |

### Already in Cargo.toml (reused)
| Library | Version | Purpose |
|---------|---------|---------|
| axum | 0.8 | Add `/metrics` route to existing health server |
| tokio | 1.47 | SIGHUP signal handler (SignalKind::hangup) |
| serde_json | 1 | Schema-to-Value conversion for jsonschema |
| Arc/RwLock | std/tokio | Shared mutable state for hot-reloadable config |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| prometheus | metrics + metrics-exporter-prometheus | Indirect layer; prometheus crate is simpler for direct exposition |
| prometheus | opentelemetry-prometheus | OTel is heavier; overkill for v1 (OTel tracing deferred to v2 OBS-V2-01) |
| jsonschema | valico | valico is unmaintained (last release 2021); jsonschema is actively maintained |
| SIGHUP only | notify (file watcher) | File watch adds a dependency; SIGHUP is sufficient and conventional for daemons |

**Installation:**
```bash
cargo add prometheus@0.14.0
cargo add jsonschema@0.42.1
```

## Architecture Patterns

### New Module Structure
```
src/
├── metrics/
│   └── mod.rs           # Registry, metric definitions, record_* helpers
├── validation/
│   └── mod.rs           # SchemaCache, validate_tool_args()
├── config/
│   ├── mod.rs           # (existing) + reload_config() function
│   ├── types.rs         # (existing)
│   └── secrets.rs       # (existing)
├── health/
│   └── server.rs        # (existing) + add /metrics route
└── gateway.rs           # (existing) + wire validation + metrics recording
```

### Pattern 1: Prometheus Registry as Shared State
**What:** Create a `Metrics` struct holding all metric objects, registered to a single `prometheus::Registry`. Pass as `Arc<Metrics>` to both the dispatch loop (for recording) and the health server (for exposition).
**When to use:** Always -- metrics must be writable from dispatch and readable from HTTP.
**Example:**
```rust
// Source: prometheus crate docs
use prometheus::{CounterVec, HistogramVec, GaugeVec, Registry, Opts, HistogramOpts};

pub struct Metrics {
    pub requests_total: CounterVec,
    pub request_duration_seconds: HistogramVec,
    pub errors_total: CounterVec,
    pub backend_healthy: GaugeVec,
    pub rate_limit_hits_total: CounterVec,
    registry: Registry,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();
        let requests_total = CounterVec::new(
            Opts::new("sentinel_requests_total", "Total tool call requests"),
            &["tool", "status"],
        ).unwrap();
        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new("sentinel_request_duration_seconds", "Tool call latency"),
            &["tool"],
        ).unwrap();
        let errors_total = CounterVec::new(
            Opts::new("sentinel_errors_total", "Total errors by type"),
            &["tool", "error_type"],
        ).unwrap();
        let backend_healthy = GaugeVec::new(
            Opts::new("sentinel_backend_healthy", "Backend health status"),
            &["backend"],
        ).unwrap();
        let rate_limit_hits_total = CounterVec::new(
            Opts::new("sentinel_rate_limit_hits_total", "Rate limit rejections"),
            &["tool"],
        ).unwrap();

        registry.register(Box::new(requests_total.clone())).unwrap();
        registry.register(Box::new(request_duration_seconds.clone())).unwrap();
        registry.register(Box::new(errors_total.clone())).unwrap();
        registry.register(Box::new(backend_healthy.clone())).unwrap();
        registry.register(Box::new(rate_limit_hits_total.clone())).unwrap();

        Self { requests_total, request_duration_seconds, errors_total, backend_healthy, rate_limit_hits_total, registry }
    }

    pub fn gather_text(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let families = self.registry.gather();
        let mut buf = Vec::new();
        encoder.encode(&families, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }
}
```

### Pattern 2: Compiled Schema Cache
**What:** After tool catalog is built, compile a `jsonschema` validator for each tool's `input_schema` and store in a `HashMap<String, jsonschema::Validator>`. Validation runs before backend dispatch.
**When to use:** For OBS-03/OBS-04 -- validate tool arguments.
**Example:**
```rust
// Source: jsonschema crate docs + rmcp Tool.input_schema
use std::collections::HashMap;
use serde_json::Value;

pub struct SchemaCache {
    validators: HashMap<String, jsonschema::Validator>,
}

impl SchemaCache {
    pub fn from_catalog(catalog: &ToolCatalog) -> Self {
        let mut validators = HashMap::new();
        for tool in catalog.all_tools() {
            let schema_value = tool.schema_as_json_value();
            match jsonschema::validator_for(&schema_value) {
                Ok(validator) => {
                    validators.insert(tool.name.to_string(), validator);
                }
                Err(e) => {
                    tracing::warn!(tool = %tool.name, error = %e, "Failed to compile schema, skipping validation for this tool");
                }
            }
        }
        Self { validators }
    }

    pub fn validate(&self, tool_name: &str, arguments: &Value) -> Result<(), Vec<String>> {
        let Some(validator) = self.validators.get(tool_name) else {
            return Ok(()); // No schema = no validation
        };
        let errors: Vec<String> = validator.iter_errors(arguments)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}
```

### Pattern 3: Hot Reload via Arc<RwLock<>> + SIGHUP
**What:** Wrap hot-reloadable config sections (KillSwitchConfig, RateLimitConfig) in `Arc<tokio::sync::RwLock<>>`. On SIGHUP, re-read and parse sentinel.toml, validate it, then swap the inner values. On failure, log error and keep previous config.
**When to use:** CONFIG-03, KILL-03 -- config reload without restart.
**Example:**
```rust
// In main.rs signal handler
let mut sighup = signal(SignalKind::hangup()).expect("SIGHUP handler");

loop {
    sighup.recv().await;
    tracing::info!("SIGHUP received, reloading config");
    match load_config_lenient(&config_path) {
        Ok(new_config) => {
            // Swap kill switch
            *kill_switch_rw.write().await = new_config.kill_switch;
            // Swap rate limits (creates new RateLimiter)
            *rate_limiter_rw.write().await = RateLimiter::new(&new_config.rate_limits);
            tracing::info!("Config reloaded successfully");
        }
        Err(e) => {
            tracing::error!(error = %e, "Config reload failed, keeping previous config");
        }
    }
}
```

### Anti-Patterns to Avoid
- **Global prometheus registry:** Use an explicit `Registry` instance, not `prometheus::default_registry()`. Explicit registries are testable and don't leak across tests.
- **Validating after dispatch:** Schema validation must happen BEFORE the backend call, in the kill_switch -> rate_limit -> RBAC -> validation -> circuit_breaker -> dispatch pipeline.
- **Reloading ALL config on SIGHUP:** Only reload mutable sections (kill_switch, rate_limits). Never hot-swap auth config, backend definitions, or postgres settings -- those require restart.
- **Blocking RwLock in async context:** Use `tokio::sync::RwLock` (not `std::sync::RwLock`) for hot-reloadable config since reads happen in async dispatch loop.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Prometheus text format | Custom metric formatter | prometheus::TextEncoder | Exact format spec compliance (comments, labels, histogram buckets, timestamp) |
| JSON Schema validation | Custom argument checker | jsonschema::Validator | JSON Schema spec has 30+ keywords, conditional schemas, $ref resolution |
| Histogram buckets | Custom bucketing logic | prometheus::HistogramOpts::linear_buckets or DEFAULT_BUCKETS | Prometheus convention, compatible with Grafana histogram_quantile() |
| Signal handling | Raw libc signal handlers | tokio::signal::unix | Async-safe, no UB, integrates with tokio event loop |

**Key insight:** Prometheus text format and JSON Schema are both specs with subtle edge cases. Hand-rolling either will produce output that works in simple cases but breaks with real Prometheus scrapers or complex tool schemas.

## Common Pitfalls

### Pitfall 1: Prometheus Label Cardinality Explosion
**What goes wrong:** Using unbounded values (client IDs, request IDs) as metric labels creates infinite time series, crashing Prometheus.
**Why it happens:** Natural instinct to track per-client metrics.
**How to avoid:** Labels must be bounded. Use tool name (bounded by catalog) and status (bounded enum: success/error/killed/rate_limited/denied/circuit_open). Never use client subject as a label.
**Warning signs:** Prometheus scrape taking >1s, `/metrics` response growing over time.

### Pitfall 2: Schema Validation Position in Pipeline
**What goes wrong:** Validating after RBAC but before circuit breaker means invalid arguments still count toward rate limits. Validating too early means unauthenticated users can probe tool schemas.
**How to avoid:** Insert validation after RBAC check, before circuit breaker check. This matches the existing enforcement order: kill switch -> rate limit -> RBAC -> **schema validation** -> circuit breaker -> backend call.
**Warning signs:** Invalid tool calls consuming rate limit tokens; schema errors not appearing in audit logs.

### Pitfall 3: Hot Reload Race Conditions
**What goes wrong:** If config is read from multiple fields (kill_switch + rate_limits) and they're separate RwLocks, a reload could be partially applied (new kill_switch, old rate_limits).
**Why it happens:** Two separate write locks acquired and released sequentially.
**How to avoid:** Bundle all hot-reloadable config into a single struct behind one `Arc<RwLock<HotConfig>>`. Single atomic swap.
**Warning signs:** Inconsistent behavior during reload (tool killed but old rate limit still applied).

### Pitfall 4: jsonschema Compilation Failure
**What goes wrong:** A backend returns a tool with an invalid or unusual `input_schema` that fails to compile.
**Why it happens:** Backend schemas are untrusted input; some MCP servers have schemas with non-standard extensions.
**How to avoid:** Log a warning and skip validation for that tool (allow through without validation). Never crash or refuse to start because of one bad schema.
**Warning signs:** Tools that worked before Phase 9 suddenly fail after adding validation.

### Pitfall 5: tokio::sync::RwLock vs std::sync::Mutex
**What goes wrong:** The existing code uses `std::sync::Mutex` for RateLimiter (decided in Phase 6 due to zero contention on single stdio transport). Hot reload needs `tokio::sync::RwLock` because the dispatch loop is async.
**Why it happens:** Mixing sync/async lock patterns.
**How to avoid:** For hot-reloadable state, use `tokio::sync::RwLock`. The RateLimiter internal `std::sync::Mutex` stays -- but the outer container holding the RateLimiter itself needs to be `Arc<tokio::sync::RwLock<RateLimiter>>` so it can be swapped on reload.
**Warning signs:** Compile errors about `Send`/`Sync` bounds or holding a `MutexGuard` across `.await`.

## Code Examples

### /metrics Endpoint Handler
```rust
// Add to health/server.rs
use prometheus::Registry;

async fn metrics_handler(
    State(metrics): State<Arc<Metrics>>,
) -> (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String) {
    let body = metrics.gather_text();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}
```

### Recording Metrics in Dispatch Loop
```rust
// After each tools/call completes:
metrics.requests_total.with_label_values(&[&tool_name, &status]).inc();
metrics.request_duration_seconds.with_label_values(&[&tool_name]).observe(latency_secs);

// On rate limit hit:
metrics.rate_limit_hits_total.with_label_values(&[&tool_name]).inc();
metrics.errors_total.with_label_values(&[&tool_name, "rate_limited"]).inc();
```

### Schema Validation in tools/call Pipeline
```rust
// In gateway.rs, after RBAC check, before circuit breaker:
if let Some(arguments) = request.params.as_ref().and_then(|p| p.get("arguments")) {
    if let Err(errors) = schema_cache.validate(name, arguments) {
        let msg = format!("Invalid arguments for tool {name}: {}", errors.join("; "));
        let resp = JsonRpcResponse::error(id.clone(), INVALID_PARAMS, msg);
        send_response(&tx, &resp).await;
        // Record in metrics + audit
        continue;
    }
}
```

### HotConfig Struct
```rust
pub struct HotConfig {
    pub kill_switch: KillSwitchConfig,
    pub rate_limiter: RateLimiter,
}

// In main.rs:
let hot_config = Arc::new(tokio::sync::RwLock::new(HotConfig {
    kill_switch: config.kill_switch,
    rate_limiter: RateLimiter::new(&config.rate_limits),
}));
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| prometheus 0.13 | prometheus 0.14.0 | 2024 | Minor API cleanup, no breaking changes |
| jsonschema compile() | jsonschema validator_for() | jsonschema 0.20+ | Renamed API, auto-detects draft version |
| signal-hook crate | tokio::signal::unix | Always (for tokio apps) | Native tokio integration, no extra dependency |

**Deprecated/outdated:**
- `jsonschema::JSONSchema::compile()` renamed to `jsonschema::validator_for()` in recent versions
- `prometheus::default_registry()` -- prefer explicit Registry for testability

## Open Questions

1. **Should `/metrics` be on the health server port (9201) or a separate port?**
   - What we know: `/health` and `/ready` are on 9201 via the existing axum health server
   - Recommendation: Add `/metrics` to the same 9201 server. Prometheus convention is a single metrics port. Keeps architecture simple.

2. **Should schema validation block tools with empty/minimal schemas (`{"type": "object", "properties": {}}`)?**
   - What we know: Many MCP tools have permissive schemas. The stub catalog uses `{"type": "object", "properties": {}}`.
   - Recommendation: Validate against the schema as-is. A permissive schema will accept anything -- that's correct behavior. Only reject when arguments actively violate the declared schema.

3. **Which config sections should be hot-reloadable?**
   - What we know: Kill switch and rate limits are the explicit requirements. Backend definitions, auth, and postgres are complex (need reconnection, re-discovery).
   - Recommendation: Hot-reload ONLY kill_switch and rate_limits. Document that backend/auth changes require restart. This matches KILL-03 and CONFIG-03 scope.

## Sources

### Primary (HIGH confidence)
- prometheus 0.14.0 docs (docs.rs/prometheus/0.14.0) -- metric types, TextEncoder, Registry API
- jsonschema 0.42.1 docs (docs.rs/jsonschema) -- validator_for(), iter_errors(), compilation API
- rmcp 0.16.0 source (local: ~/.cargo/registry) -- Tool.input_schema is Arc<JsonObject>, schema_as_json_value() returns Value::Object
- tokio::signal::unix docs -- SignalKind::hangup(), same API as existing SIGTERM/SIGINT handlers

### Secondary (MEDIUM confidence)
- Prometheus text format spec -- Content-Type "text/plain; version=0.0.4"
- axum-prometheus crate docs -- confirmed middleware approach is for HTTP metrics, not our use case

### Tertiary (LOW confidence)
- None -- all findings verified against official docs or source code

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- prometheus 0.14.0 and jsonschema 0.42.1 are stable, well-documented crates verified via cargo search and docs.rs
- Architecture: HIGH -- patterns follow existing codebase conventions (Arc shared state, axum routes, dispatch pipeline)
- Pitfalls: HIGH -- derived from reading actual source code and understanding the existing enforcement order
- Hot reload: HIGH -- tokio::signal already used in the codebase, RwLock pattern is standard

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable domain, no fast-moving dependencies)
