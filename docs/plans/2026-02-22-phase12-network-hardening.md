# Phase 12: Network Hardening — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Harden Sentinel Gateway's Docker containers, protect the metrics endpoint with bearer auth, and add shared-secret authentication between Sentinel and its HTTP sidecar backends.

**Architecture:** Three independent layers: (1) Docker compose security directives, (2) Axum middleware on `/metrics`, (3) `X-Sentinel-Auth` header injected by Rust HTTP client and validated by Express middleware in sidecars.

**Tech Stack:** Rust (axum, reqwest), Node.js (Express), Docker Compose

---

### Task 1: Docker Container Hardening

**Files:**
- Modify: `/home/lwb3/sentinel-gateway/docker-compose.yml`

**Step 1: Add security directives to all services**

Edit `docker-compose.yml` to add hardening to each service:

For `postgres`:
```yaml
  postgres:
    image: postgres:16-alpine
    container_name: sentinel-postgres
    restart: unless-stopped
    read_only: true
    tmpfs:
      - /tmp
      - /run/postgresql
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    cap_add:
      - SETUID
      - SETGID
    pids_limit: 100
    deploy:
      resources:
        limits:
          memory: 256M
    # ... rest unchanged
```

For `mcp-n8n` add (before existing `deploy:` block):
```yaml
    read_only: true
    tmpfs:
      - /tmp
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    pids_limit: 100
```

For `mcp-sqlite` add same block as mcp-n8n.

**Step 2: Make sentinelnet internal**

```yaml
networks:
  sentinelnet:
    driver: bridge
    internal: true
```

**Step 3: Test containers start correctly**

Run: `docker compose -f /home/lwb3/sentinel-gateway/docker-compose.yml up -d --force-recreate`
Expected: All 3 containers reach healthy status.

Run: `docker ps --filter "name=sentinel-postgres" --filter "name=mcp-n8n" --filter "name=mcp-sqlite" --format "{{.Names}}: {{.Status}}"`
Expected: All show `Up ... (healthy)`

**Step 4: Verify read-only works (no write errors in logs)**

Run: `docker logs mcp-n8n --tail 10` and `docker logs mcp-sqlite --tail 10` and `docker logs sentinel-postgres --tail 10`
Expected: No permission-denied errors.

**Step 5: Commit**

```bash
git add docker-compose.yml
git commit -m "harden(phase-12): add read-only, cap-drop, no-new-privileges, internal network"
```

---

### Task 2: Add HEALTH_TOKEN env var

**Files:**
- Modify: `/home/lwb3/sentinel-gateway/.env` (add `HEALTH_TOKEN`)
- Modify: `/home/lwb3/sentinel-gateway/.env.example` (add placeholder)

**Step 1: Generate and add token**

Run: `python3 -c "import secrets; print(secrets.token_urlsafe(32))"` to generate a token.

Add to `.env`:
```
HEALTH_TOKEN=<generated-value>
```

**Step 2: Add placeholder to .env.example**

Add line:
```
# Bearer token for /metrics endpoint
HEALTH_TOKEN=change-me-generate-random-token
```

**Step 3: Commit .env.example only**

```bash
git add .env.example
git commit -m "config(phase-12): add HEALTH_TOKEN placeholder to .env.example"
```

---

### Task 3: Protect /metrics with bearer auth

**Files:**
- Modify: `/home/lwb3/sentinel-gateway/src/health/server.rs`

**Step 1: Write failing tests for metrics auth**

Add these tests to the `#[cfg(test)] mod tests` block in `src/health/server.rs`:

```rust
#[tokio::test]
async fn metrics_requires_auth_when_token_set() {
    let metrics = Arc::new(Metrics::new());
    metrics.record_request("echo", "success", 0.01);
    let app = build_health_router(make_health_map(), Some(metrics), Some("test-token".to_string()));
    let req = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn metrics_accessible_with_valid_token() {
    let metrics = Arc::new(Metrics::new());
    metrics.record_request("echo", "success", 0.01);
    let app = build_health_router(make_health_map(), Some(metrics), Some("test-token".to_string()));
    let req = Request::builder()
        .uri("/metrics")
        .header("Authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_rejects_wrong_token() {
    let metrics = Arc::new(Metrics::new());
    metrics.record_request("echo", "success", 0.01);
    let app = build_health_router(make_health_map(), Some(metrics), Some("test-token".to_string()));
    let req = Request::builder()
        .uri("/metrics")
        .header("Authorization", "Bearer wrong-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn metrics_open_when_no_token_configured() {
    let metrics = Arc::new(Metrics::new());
    metrics.record_request("echo", "success", 0.01);
    let app = build_health_router(make_health_map(), Some(metrics), None);
    let req = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib health::server::tests -- --nocapture` (with `dangerouslyDisableSandbox: true`)
Expected: Compilation error — `build_health_router` signature mismatch (missing `health_token` param).

**Step 3: Implement metrics auth**

Update `HealthAppState` to include the token:
```rust
#[derive(Clone)]
pub struct HealthAppState {
    pub health_map: BackendHealthMap,
    pub metrics: Option<Arc<Metrics>>,
    pub health_token: Option<String>,
}
```

Update `metrics_handler` to check auth:
```rust
async fn metrics_handler(
    State(state): State<HealthAppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(ref expected) = state.health_token {
        let provided = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match provided {
            Some(token) if token == expected => {}
            _ => return (StatusCode::UNAUTHORIZED, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "Unauthorized".to_string()),
        }
    }
    match &state.metrics {
        Some(m) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
            m.gather_text(),
        ),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Metrics not enabled".to_string(),
        ),
    }
}
```

Update `build_health_router` signature:
```rust
pub fn build_health_router(
    health_map: BackendHealthMap,
    metrics: Option<Arc<Metrics>>,
    health_token: Option<String>,
) -> Router {
    let state = HealthAppState {
        health_map,
        metrics,
        health_token,
    };
    // ... router unchanged
}
```

Update `run_health_server` signature to pass `health_token`:
```rust
pub async fn run_health_server(
    addr: &str,
    health_map: BackendHealthMap,
    metrics: Option<Arc<Metrics>>,
    health_token: Option<String>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let app = build_health_router(health_map, metrics, health_token);
    // ... rest unchanged
}
```

**Step 4: Fix all callers of build_health_router/run_health_server**

Find callers in `src/main.rs` — update to pass `std::env::var("HEALTH_TOKEN").ok()`.

Update existing tests in `server.rs` — the helper `build_app` and existing test calls need to pass `None` for the token param:
```rust
fn build_app(health_map: BackendHealthMap) -> Router {
    build_health_router(health_map, None, None)
}
```

And update `test_metrics_endpoint_returns_prometheus_text`:
```rust
let app = build_health_router(make_health_map(), Some(metrics), None);
```

And update `test_metrics_endpoint_returns_404_when_disabled`:
```rust
let app = build_health_router(make_health_map(), None, None);
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --lib health::server::tests -- --nocapture`
Expected: All tests PASS (existing + 4 new).

**Step 6: Commit**

```bash
git add src/health/server.rs
git commit -m "feat(phase-12): add bearer token auth to /metrics endpoint"
```

---

### Task 4: Add BACKEND_SHARED_SECRET env var

**Files:**
- Modify: `/home/lwb3/sentinel-gateway/.env` (add `BACKEND_SHARED_SECRET`)
- Modify: `/home/lwb3/sentinel-gateway/.env.example` (add placeholder)

**Step 1: Generate and add secret**

Run: `python3 -c "import secrets; print(secrets.token_urlsafe(32))"` to generate.

Add to `.env`:
```
BACKEND_SHARED_SECRET=<generated-value>
```

**Step 2: Add placeholder to .env.example**

Add line:
```
# Shared secret for authenticating with HTTP sidecar backends
BACKEND_SHARED_SECRET=change-me-generate-random-secret
```

**Step 3: Add to docker-compose.yml environment for sidecars**

Add `BACKEND_SHARED_SECRET=${BACKEND_SHARED_SECRET}` to the `environment:` block of both `mcp-n8n` and `mcp-sqlite` services.

**Step 4: Commit**

```bash
git add .env.example docker-compose.yml
git commit -m "config(phase-12): add BACKEND_SHARED_SECRET to env and sidecar config"
```

---

### Task 5: Inject X-Sentinel-Auth header in HTTP backend

**Files:**
- Modify: `/home/lwb3/sentinel-gateway/src/backend/http.rs`

**Step 1: Write failing test**

Add to `src/backend/http.rs` (create test module if none exists):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_auth_secret() {
        let client = build_http_client().unwrap();
        let config = BackendConfig {
            name: "test".to_string(),
            backend_type: crate::config::types::BackendType::Http,
            url: Some("http://localhost:3000".to_string()),
            command: None,
            args: vec![],
            env: std::collections::HashMap::new(),
            timeout_secs: 60,
            retries: 3,
            restart_on_exit: false,
            max_restarts: 5,
            health_interval_secs: 300,
            circuit_breaker_threshold: 5,
            circuit_breaker_recovery_secs: 30,
        };
        let backend = HttpBackend::new(client, &config, Some("my-secret".to_string()));
        assert_eq!(backend.auth_secret.as_deref(), Some("my-secret"));
    }

    #[test]
    fn new_without_auth_secret() {
        let client = build_http_client().unwrap();
        let config = BackendConfig {
            name: "test".to_string(),
            backend_type: crate::config::types::BackendType::Http,
            url: Some("http://localhost:3000".to_string()),
            command: None,
            args: vec![],
            env: std::collections::HashMap::new(),
            timeout_secs: 60,
            retries: 3,
            restart_on_exit: false,
            max_restarts: 5,
            health_interval_secs: 300,
            circuit_breaker_threshold: 5,
            circuit_breaker_recovery_secs: 30,
        };
        let backend = HttpBackend::new(client, &config, None);
        assert!(backend.auth_secret.is_none());
    }
}
```

**Step 2: Run tests to verify failure**

Run: `cargo test --lib backend::http::tests -- --nocapture`
Expected: Compilation error — `HttpBackend::new` takes 2 args, 3 given.

**Step 3: Add auth_secret field and inject header**

Add field to `HttpBackend`:
```rust
pub struct HttpBackend {
    client: Client,
    url: String,
    timeout: Duration,
    max_retries: u32,
    auth_secret: Option<String>,
}
```

Update `HttpBackend::new` to accept the secret:
```rust
pub fn new(client: Client, config: &BackendConfig, auth_secret: Option<String>) -> Self {
    // ... existing url logic ...
    Self {
        client,
        url,
        timeout: Duration::from_secs(config.timeout_secs),
        max_retries: config.retries,
        auth_secret,
    }
}
```

In `send()`, add the auth header conditionally. Inside the async block, after `.header("Accept", ...)`:
```rust
let mut req = client
    .post(&url)
    .header("Content-Type", "application/json")
    .header("Accept", "application/json, text/event-stream")
    .timeout(timeout);

if let Some(ref secret) = auth_secret {
    req = req.header("X-Sentinel-Auth", secret.as_str());
}

let response = req
    .body(body)
    .send()
    .await
    .map_err(BackendError::Request)?;
```

Note: `auth_secret` needs to be cloned into the closure alongside `client`, `url`, `body`. Add:
```rust
let auth_secret = self.auth_secret.clone();
```
before the `retry_with_backoff` call, and clone it again inside the outer closure:
```rust
let auth_secret = auth_secret.clone();
```

**Step 4: Fix all callers of HttpBackend::new**

Search for `HttpBackend::new` in the codebase. Pass `std::env::var("BACKEND_SHARED_SECRET").ok()` from wherever backends are constructed (likely `src/gateway.rs` or `src/main.rs`).

**Step 5: Run tests to verify pass**

Run: `cargo test --lib backend::http::tests -- --nocapture`
Expected: PASS

Run: `cargo test` (full suite)
Expected: All 138+ tests PASS

**Step 6: Commit**

```bash
git add src/backend/http.rs src/gateway.rs src/main.rs
git commit -m "feat(phase-12): inject X-Sentinel-Auth header on HTTP backend requests"
```

---

### Task 6: Add auth middleware to mcp-n8n sidecar

**Files:**
- Modify: `/home/lwb3/n8n-mcp-server/index.js`

**Step 1: Add auth middleware before routes**

Insert after `const app = createMcpExpressApp(...)` and before `app.post("/mcp", ...)`:

```javascript
// Shared secret auth — skip for /health (Docker healthchecks)
const BACKEND_SECRET = process.env.BACKEND_SHARED_SECRET;
if (BACKEND_SECRET) {
  app.use((req, res, next) => {
    if (req.path === "/health") return next();
    if (req.headers["x-sentinel-auth"] !== BACKEND_SECRET) {
      return res.status(401).json({ error: "Unauthorized" });
    }
    next();
  });
}
```

**Step 2: Test locally (manual)**

Run: `docker compose -f /home/lwb3/sentinel-gateway/docker-compose.yml up -d --build mcp-n8n`
Run: `curl -s http://127.0.0.1:3001/health` — Expected: 200 (health is exempt)
Run: `curl -s -X POST http://127.0.0.1:3001/mcp` — Expected: 401 Unauthorized
Run: `curl -s -X POST -H "X-Sentinel-Auth: <actual-secret>" http://127.0.0.1:3001/mcp` — Expected: non-401 response (may be 400 for missing body, but not 401)

**Step 3: Commit**

```bash
cd /home/lwb3/n8n-mcp-server && git add index.js && git commit -m "feat: add X-Sentinel-Auth shared secret validation"
```

---

### Task 7: Add auth middleware to mcp-sqlite sidecar

**Files:**
- Modify: `/home/lwb3/sqlite-mcp-server/index.js`

**Step 1: Add auth middleware before routes**

Insert after `const app = createMcpExpressApp(...)` and before `app.post("/mcp", ...)`:

```javascript
// Shared secret auth — skip for /health (Docker healthchecks)
const BACKEND_SECRET = process.env.BACKEND_SHARED_SECRET;
if (BACKEND_SECRET) {
  app.use((req, res, next) => {
    if (req.path === "/health") return next();
    if (req.headers["x-sentinel-auth"] !== BACKEND_SECRET) {
      return res.status(401).json({ error: "Unauthorized" });
    }
    next();
  });
}
```

**Step 2: Test locally (manual)**

Run: `docker compose -f /home/lwb3/sentinel-gateway/docker-compose.yml up -d --build mcp-sqlite`
Run: `curl -s http://127.0.0.1:3002/health` — Expected: 200
Run: `curl -s -X POST http://127.0.0.1:3002/mcp` — Expected: 401

**Step 3: Commit**

```bash
cd /home/lwb3/sqlite-mcp-server && git add index.js && git commit -m "feat: add X-Sentinel-Auth shared secret validation"
```

---

### Task 8: Full integration test

**Files:** None (verification only)

**Step 1: Rebuild and restart all sidecars**

Run: `docker compose -f /home/lwb3/sentinel-gateway/docker-compose.yml up -d --build --force-recreate`
Expected: All 3 containers healthy.

**Step 2: Build sentinel gateway**

Run: `cd /home/lwb3/sentinel-gateway && cargo build --release`
Expected: Compiles successfully.

**Step 3: Verify MCP tools still work end-to-end**

Use sentinel MCP tools from Claude Code:
- `sqlite_databases` — should return database list
- `list_workflows` — should return n8n workflows
- `browser_snapshot` — should work (stdio backend, unaffected)

**Step 4: Verify unauthenticated access is blocked**

Run: `curl -s -X POST http://127.0.0.1:3001/mcp -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'`
Expected: 401 Unauthorized

Run: `curl -s http://127.0.0.1:9201/metrics`
Expected: 401 Unauthorized (or whatever HEALTH_TOKEN requires)

Run: `curl -s http://127.0.0.1:9201/health`
Expected: 200 OK (no auth needed)

**Step 5: Verify Docker hardening**

Run: `docker inspect sentinel-postgres --format '{{.HostConfig.ReadonlyRootfs}}'`
Expected: `true`

Run: `docker inspect mcp-n8n --format '{{.HostConfig.CapDrop}}'`
Expected: `[ALL]`

Run: `docker inspect mcp-sqlite --format '{{.HostConfig.SecurityOpt}}'`
Expected: `[no-new-privileges]`

**Step 6: Final commit**

```bash
cd /home/lwb3/sentinel-gateway && git add -A && git commit -m "docs(phase-12): mark network hardening complete"
```
