# Phase 7: Health & Reliability - Research

**Researched:** 2026-02-22
**Domain:** HTTP health endpoints, circuit breakers, graceful shutdown (Rust/Tokio)
**Confidence:** HIGH

## Summary

Phase 7 adds operational health infrastructure to the gateway: HTTP liveness/readiness probes, periodic backend health checks, circuit breaker per backend, and graceful shutdown. The gateway currently runs as a pure stdio process with no HTTP listener -- this phase adds a lightweight HTTP server alongside the existing stdio transport for health probes only.

The architecture is straightforward: spawn an axum HTTP server on a configurable port (default `127.0.0.1:9201`) serving `/health` and `/ready`. A background task periodically pings each HTTP backend and tracks up/down state in a shared `Arc<RwLock<HashMap>>`. The circuit breaker wraps each backend with failure counting and state transitions (closed/open/half-open). Graceful shutdown uses `tokio::signal` + `CancellationToken` to coordinate draining the dispatch loop, flushing audit logs, and stopping the health server.

**Primary recommendation:** Use axum 0.8 for the health HTTP server, hand-roll the circuit breaker (it's ~60 lines for our use case), and use tokio's built-in signal + CancellationToken for shutdown coordination.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| HEALTH-01 | `/health` endpoint (liveness -- gateway process is running) | Axum route returning 200 + JSON body. No backend checks needed -- if the HTTP server responds, the process is alive. |
| HEALTH-02 | `/ready` endpoint (readiness -- at least one backend reachable) | Axum route reading shared `BackendHealthMap`. Returns 200 if any backend is healthy, 503 otherwise. |
| HEALTH-03 | Periodic backend health pinging and status tracking | Background tokio task using `tokio::time::interval`. Sends MCP `ping` request to each HTTP backend. Stores results in `Arc<RwLock<BackendHealthMap>>`. |
| HEALTH-04 | Circuit breaker per backend (open after N failures, half-open probe, close on success) | Per-backend `CircuitBreaker` struct with atomic state. Integrates into `handle_tools_call` -- check breaker before sending to backend, record success/failure after. |
| HEALTH-05 | Graceful shutdown on SIGTERM (drain in-flight, terminate stdio children, flush audit) | `tokio::signal::unix::signal(SignalKind::terminate())` + `CancellationToken`. Shutdown sequence: cancel token -> dispatch loop exits -> drop audit_tx -> writer drains remaining entries -> exit. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| axum | 0.8 | HTTP server for health endpoints | Official tokio ecosystem, thin over hyper, minimal overhead for 2-3 routes |
| tokio-util | 0.7 | `CancellationToken` for shutdown coordination | Official tokio utility, recommended in tokio shutdown guide |

### Already in Cargo.toml (no new deps needed)
| Library | Version | Purpose |
|---------|---------|---------|
| tokio | 1.47 (features = ["full"]) | Runtime, signals (`tokio::signal::unix`), intervals, select |
| reqwest | 0.12 | Already used for backend HTTP calls -- reuse for health pings |
| serde_json | 1 | JSON response bodies |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| axum | Raw hyper 1.x | More boilerplate for routing, no benefit for 2 routes |
| axum | tiny_http | Blocking, doesn't integrate with tokio runtime |
| Hand-rolled circuit breaker | failsafe-rs / tower-circuitbreaker | Over-engineered for 2-3 backends; tower integration adds complexity we don't need |
| tokio-util CancellationToken | Manual broadcast channel | CancellationToken is purpose-built, zero-cost when not cancelled |

**Installation:**
```bash
cargo add axum@0.8 tokio-util@0.7
```

## Architecture Patterns

### New Module Structure
```
src/
  health/
    mod.rs          # pub mod server; pub mod checker; pub mod circuit_breaker;
    server.rs       # Axum HTTP server (/health, /ready)
    checker.rs      # Background health check loop
    circuit_breaker.rs  # Per-backend circuit breaker
```

### Pattern 1: Shared Health State
**What:** A `BackendHealthMap` shared between the health checker (writer) and the HTTP endpoints + dispatch loop (readers).
**When to use:** Always -- this is the central coordination point.
**Example:**
```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct BackendHealth {
    pub healthy: bool,
    pub last_check: std::time::Instant,
    pub consecutive_failures: u32,
}

pub type BackendHealthMap = Arc<RwLock<HashMap<String, BackendHealth>>>;
```

### Pattern 2: Circuit Breaker State Machine
**What:** Three-state machine (Closed/Open/HalfOpen) per backend, tracking consecutive failures.
**When to use:** Wraps every backend call in the dispatch loop.
**Example:**
```rust
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use std::sync::Mutex;

#[derive(Debug)]
pub struct CircuitBreaker {
    state: AtomicU8,          // 0=Closed, 1=Open, 2=HalfOpen
    failure_count: AtomicU32,
    failure_threshold: u32,   // from config, e.g. 5
    recovery_timeout: Duration, // from config, e.g. 30s
    last_failure: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn allow_request(&self) -> bool {
        match self.state.load(Ordering::Relaxed) {
            0 => true, // Closed -- allow
            1 => {     // Open -- check if recovery timeout elapsed
                let last = self.last_failure.lock().unwrap();
                if last.map_or(false, |t| t.elapsed() >= self.recovery_timeout) {
                    self.state.store(2, Ordering::Relaxed); // -> HalfOpen
                    true // allow one probe
                } else {
                    false // still open
                }
            }
            2 => true, // HalfOpen -- allow probe request
            _ => false,
        }
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        self.state.store(0, Ordering::Relaxed); // -> Closed
    }

    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last_failure.lock().unwrap() = Some(Instant::now());
        if count >= self.failure_threshold {
            self.state.store(1, Ordering::Relaxed); // -> Open
        }
    }
}
```

### Pattern 3: Graceful Shutdown with CancellationToken
**What:** Coordinate shutdown across dispatch loop, health server, health checker, and audit writer.
**When to use:** On SIGTERM or SIGINT.
**Example:**
```rust
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;

async fn shutdown_signal(token: CancellationToken) {
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
        _ = sigint.recv() => tracing::info!("Received SIGINT"),
    }

    token.cancel();
}
```

### Pattern 4: Axum Health Server
**What:** Lightweight HTTP server serving only health probes.
**When to use:** Runs alongside stdio transport on a separate port.
**Example:**
```rust
use axum::{routing::get, Router, Json, extract::State};
use serde_json::json;

async fn liveness() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn readiness(
    State(health_map): State<BackendHealthMap>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    let map = health_map.read().await;
    let any_healthy = map.values().any(|h| h.healthy);
    if any_healthy {
        (axum::http::StatusCode::OK, Json(json!({"status": "ready"})))
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "not_ready"})))
    }
}

pub async fn run_health_server(
    addr: &str,
    health_map: BackendHealthMap,
    cancel: CancellationToken,
) {
    let app = Router::new()
        .route("/health", get(liveness))
        .route("/ready", get(readiness))
        .with_state(health_map);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
        .unwrap();
}
```

### Anti-Patterns to Avoid
- **Health check on the same stdio transport:** Health probes must be HTTP, not MCP-over-stdio. Docker/k8s probe runners send HTTP GET, not JSON-RPC.
- **Blocking health checks in the dispatch loop:** Health pings must run in a background task, never inline with request processing.
- **Circuit breaker without half-open state:** Skipping half-open means a backend that recovers stays permanently blocked until restart.
- **Unbounded failure counters:** Use `AtomicU32` -- if it overflows, the circuit should already be open. Reset on success.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP server | Raw TCP parsing | axum 0.8 | Correct HTTP/1.1 handling, keep-alive, proper header parsing |
| Shutdown coordination | Manual broadcast + flags | CancellationToken (tokio-util) | Purpose-built, composable, zero overhead when not triggered |
| Signal handling | libc::signal | tokio::signal::unix | Async-safe, integrates with tokio select!, handles edge cases |

**Key insight:** The circuit breaker IS worth hand-rolling here -- our use case (2-3 backends, simple threshold) doesn't justify a dependency. But HTTP serving and signal handling have enough edge cases to warrant proven libraries.

## Common Pitfalls

### Pitfall 1: Health Server Port Conflict
**What goes wrong:** Health server binds to the same address as another service.
**Why it happens:** Default port not configurable, or collides with the gateway's conceptual "listen" address.
**How to avoid:** Add `health_listen` field to `GatewayConfig` (default `127.0.0.1:9201`), separate from the existing `listen` field (which is currently unused for actual listening since transport is stdio).
**Warning signs:** "address already in use" error on startup.

### Pitfall 2: Shutdown Ordering
**What goes wrong:** Audit entries are lost because the writer task is cancelled before it can drain.
**Why it happens:** All tasks cancelled simultaneously, writer doesn't get a chance to flush.
**How to avoid:** Shutdown sequence must be ordered: (1) cancel dispatch loop, (2) drop `audit_tx` sender, (3) await audit writer task (it already drains on channel close -- this was future-proofed in Phase 5).
**Warning signs:** "Audit writer shutting down" log appears before final tool call audit entries.

### Pitfall 3: Health Check Flooding Backends
**What goes wrong:** Health checks overwhelm backends with ping requests.
**Why it happens:** Interval too aggressive (e.g., every 1 second with 10 backends).
**How to avoid:** Default interval of 30 seconds. Configurable via `health_interval_secs` (already in `BackendConfig`). Stagger checks across backends (don't ping all at once).
**Warning signs:** Backend logs show excessive ping traffic.

### Pitfall 4: Circuit Breaker Blocks All Requests During Transient Failure
**What goes wrong:** A single timeout opens the circuit, blocking ALL requests to that backend.
**Why it happens:** Failure threshold too low (e.g., 1).
**How to avoid:** Default threshold of 5 consecutive failures. Make configurable. Only count consecutive failures (reset on any success).
**Warning signs:** Users report tools suddenly unavailable after brief backend hiccup.

### Pitfall 5: CancellationToken Not Propagated to Dispatch Loop
**What goes wrong:** SIGTERM received but dispatch loop blocks on `rx.recv()` indefinitely.
**Why it happens:** `rx.recv()` only returns `None` when all senders are dropped, which doesn't happen on signal.
**How to avoid:** Use `tokio::select!` in the dispatch loop: `select! { line = rx.recv() => ..., _ = token.cancelled() => break }`.
**Warning signs:** Process doesn't exit after SIGTERM, requires SIGKILL.

## Code Examples

### Health Check Background Task
```rust
use tokio::time::{interval, Duration};

pub async fn health_checker(
    backends: Vec<(String, HttpBackend)>,
    health_map: BackendHealthMap,
    cancel: CancellationToken,
    interval_secs: u64,
) {
    let mut tick = interval(Duration::from_secs(interval_secs));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                for (name, backend) in &backends {
                    let ping = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "ping"
                    });
                    let body = serde_json::to_string(&ping).unwrap();
                    let healthy = backend.send(&body).await.is_ok();

                    let mut map = health_map.write().await;
                    let entry = map.entry(name.clone()).or_insert(BackendHealth {
                        healthy: false,
                        last_check: std::time::Instant::now(),
                        consecutive_failures: 0,
                    });
                    entry.last_check = std::time::Instant::now();
                    if healthy {
                        entry.healthy = true;
                        entry.consecutive_failures = 0;
                    } else {
                        entry.consecutive_failures += 1;
                        entry.healthy = false;
                    }
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Health checker shutting down");
                break;
            }
        }
    }
}
```

### Modified Main with Shutdown Coordination
```rust
// In main.rs (conceptual):
let cancel = CancellationToken::new();

// Spawn signal listener
let cancel_clone = cancel.clone();
tokio::spawn(shutdown_signal(cancel_clone));

// Spawn health HTTP server
let cancel_clone = cancel.clone();
tokio::spawn(run_health_server(&config.gateway.health_listen, health_map.clone(), cancel_clone));

// Spawn health checker
let cancel_clone = cancel.clone();
tokio::spawn(health_checker(backends_list, health_map.clone(), cancel_clone, 30));

// Run dispatch loop (now accepts cancel token)
run_dispatch(rx, tx, ..., cancel.clone()).await?;

// After dispatch exits, drop audit_tx to signal writer
drop(audit_tx);

// Wait for audit writer to drain
audit_handle.await?;

tracing::info!("Shutdown complete");
```

### Circuit Breaker Integration in Dispatch
```rust
// In handle_tools_call, before sending to backend:
if !circuit_breakers[&backend_name].allow_request() {
    return JsonRpcResponse::error(
        client_id,
        INTERNAL_ERROR, // or a dedicated CIRCUIT_OPEN_ERROR
        format!("Backend circuit open: {backend_name}"),
    );
}

// After backend response:
match backend.send(&body).await {
    Ok(response) => {
        circuit_breakers[&backend_name].record_success();
        // ... process response
    }
    Err(e) => {
        circuit_breakers[&backend_name].record_failure();
        // ... return error
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| hyper 0.14 direct | axum 0.8 (thin over hyper 1.x) | Jan 2025 | Simpler routing, same performance |
| Manual broadcast for shutdown | CancellationToken (tokio-util) | Stable since tokio-util 0.7 | Purpose-built, composable |
| External circuit breaker crate | Hand-rolled for simple cases | Always valid for <5 backends | Fewer dependencies, easier to reason about |

**Deprecated/outdated:**
- `tokio-signal` crate: Merged into `tokio::signal` module. Use `tokio::signal::unix::signal()` directly.
- `hyper::Server` (0.14 style): hyper 1.0 removed the built-in server. Use `axum::serve` or `hyper-util`.

## Open Questions

1. **Circuit breaker error code**
   - What we know: Kill switch uses -32005, rate limit uses -32006
   - What's unclear: Should circuit-open use a new dedicated code (e.g., -32007) or reuse INTERNAL_ERROR (-32603)?
   - Recommendation: Use a dedicated -32007 CIRCUIT_OPEN_ERROR for observability, consistent with the project's pattern of distinct error codes per rejection type.

2. **Health check for stdio backends**
   - What we know: Phase 8 adds stdio backends. Health checks for stdio would send ping over stdin.
   - What's unclear: Should Phase 7 stub out stdio health checking, or leave it entirely to Phase 8?
   - Recommendation: Phase 7 health checker should only handle HTTP backends. Phase 8 will extend it for stdio. The `BackendHealthMap` is transport-agnostic, so no refactoring needed later.

## Sources

### Primary (HIGH confidence)
- [Tokio Graceful Shutdown Guide](https://tokio.rs/tokio/topics/shutdown) - CancellationToken pattern, task draining
- [Axum 0.8 docs](https://docs.rs/axum/latest/axum/) - Latest API, Router, State extraction, graceful_shutdown
- [tokio::signal::unix docs](https://docs.rs/tokio/latest/tokio/signal/unix/struct.Signal.html) - SIGTERM handling

### Secondary (MEDIUM confidence)
- [Axum 0.8.0 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) - Version confirmation, breaking changes from 0.7
- Existing codebase analysis (src/main.rs, src/gateway.rs, src/audit/writer.rs) - Shutdown drain already implemented for audit writer

### Tertiary (LOW confidence)
- None -- all findings verified with official sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - axum and tokio-util are official tokio ecosystem, well-documented
- Architecture: HIGH - patterns follow standard tokio practices (CancellationToken, select!, shared state)
- Pitfalls: HIGH - derived from actual codebase analysis (audit drain ordering, dispatch loop blocking)
- Circuit breaker: HIGH - simple state machine pattern, well-understood, no exotic requirements

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable ecosystem, no fast-moving dependencies)
