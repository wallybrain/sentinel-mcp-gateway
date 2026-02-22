# Architecture: v1.1 Deploy, Cutover, and Monitoring

**Domain:** MCP Gateway deployment + observability
**Researched:** 2026-02-22
**Confidence:** HIGH (based on existing codebase, VPS inventory, Prometheus/Grafana docs)

## Current State Overview

```
Claude Code
    |
    | stdio (JSON-RPC)
    v
Docker wrapper container (--network=host)
    |
    | HTTP POST to http://localhost:9200 (Bearer JWT)
    v
ContextForge Gateway (Python/FastAPI, port 9200)
    |
    +---> mcp-n8n:3000 (mcpnet)
    +---> mcp-sqlite:3000 (mcpnet)

+ 5 ungoverned stdio servers (context7, firecrawl, exa, playwright, sequential-thinking)
  launched directly by Claude Code as child processes
```

**Containers today:** 14 total, ~430 MB for ContextForge stack (gateway + postgres + redis + mcp-n8n + mcp-sqlite).

## Target State (v1.1)

```
Claude Code
    |
    | stdio (JSON-RPC)
    v
Sentinel Gateway (Rust, Docker)
    |  listen: 127.0.0.1:9200 (MCP transport, Streamable HTTP)
    |  health: 127.0.0.1:9201 (/health, /ready, /metrics)
    |
    +---> mcp-n8n:3000 (HTTP backend, sentinelnet)
    +---> mcp-sqlite:3000 (HTTP backend, sentinelnet)
    +---> context7 (managed stdio child)
    +---> firecrawl (managed stdio child)
    +---> exa (managed stdio child)
    +---> sequential-thinking (managed stdio child)
    +---> playwright (managed stdio child)
    |
    | Prometheus scrape
    v
Prometheus (port 9090, Docker internal)
    |
    | PromQL queries
    v
Grafana (port 127.0.0.1:3100, Docker)
    |
    | (optional: Caddy reverse proxy for browser access)
```

## Component Map: New vs Modified vs Removed

### New Containers (3)

| Container | Image | Port | Network | RAM | Purpose |
|-----------|-------|------|---------|-----|---------|
| `sentinel-gateway` | Built from `./Dockerfile` | `127.0.0.1:9200` (MCP), `127.0.0.1:9201` (health/metrics) | `sentinelnet` + `n8nnet` | ~50 MB | MCP gateway |
| `sentinel-postgres` | `postgres:16-alpine` | internal `5432` | `sentinelnet` | ~50 MB | Audit logs |
| `prometheus` | `prom/prometheus:latest` | internal `9090` | `sentinelnet` | ~80 MB | Metrics collection |
| `grafana` | `grafana/grafana-oss:latest` | `127.0.0.1:3100` | `sentinelnet` | ~60 MB | Dashboards |

**Net RAM change:** ContextForge stack (~430 MB) removed, Sentinel stack (~240 MB) added. Net savings: ~190 MB.

### Modified Components

| Component | Change | Why |
|-----------|--------|-----|
| `~/.claude/settings.json` | MCP server config points to Sentinel instead of ContextForge wrapper | Cutover |
| `mcp-n8n` | Move from `mcpnet` to `sentinelnet` (plus keep `n8nnet`) | New network for Sentinel |
| `mcp-sqlite` | Move from `mcpnet` to `sentinelnet` | New network for Sentinel |
| n8n health workflow | Add Sentinel health check (HTTP GET `http://sentinel-gateway:9201/health`) | Monitoring |

### Removed Containers (3)

| Container | Why |
|-----------|-----|
| `mcp-context-forge-gateway-1` | Replaced by `sentinel-gateway` |
| `mcp-context-forge-postgres-1` | Replaced by `sentinel-postgres` |
| `mcp-context-forge-redis-1` | Not needed (Sentinel has no Redis dependency) |

### Unchanged

All web services (Caddy, wallybrain-music, solitaire, Authelia), automation (n8n, chiasm), and existing monitoring (cpu-server, system-stats, container-health) remain untouched.

## Detailed Data Flows

### 1. MCP Request Flow (post-cutover)

```
Claude Code
    |
    | stdio: launches sentinel-gateway container as child process
    | (or: Docker wrapper POSTs to http://localhost:9200)
    v
sentinel-gateway container
    |
    | 1. JWT validation (HS256)
    | 2. RBAC check (tool + role)
    | 3. Rate limit (token bucket)
    | 4. Kill switch check
    | 5. Audit log (async write to sentinel-postgres)
    |
    +--[HTTP]---> mcp-n8n:3000 ---> n8n:5678 (via n8nnet bridge)
    +--[HTTP]---> mcp-sqlite:3000 ---> /data/*.db (volume mount)
    +--[stdio]--> context7 (child process inside container)
    +--[stdio]--> firecrawl (child process inside container)
    +--[stdio]--> exa (child process inside container)
    +--[stdio]--> sequential-thinking (child process inside container)
    +--[stdio]--> playwright (child process inside container)
```

### 2. Prometheus Metrics Flow

```
prometheus container (every 15s)
    |
    | HTTP GET http://sentinel-gateway:9201/metrics
    | (Docker internal network, no auth needed)
    v
sentinel-gateway /metrics endpoint
    |
    | Returns Prometheus text format:
    |   sentinel_requests_total{tool="...", status="..."}
    |   sentinel_request_duration_seconds{tool="..."}
    |   sentinel_errors_total{tool="...", error_type="..."}
    |   sentinel_backend_healthy{backend="..."}
    |   sentinel_rate_limit_hits_total{tool="..."}
    v
prometheus stores in TSDB (15-day retention, ~100 MB)
    |
    | PromQL queries from Grafana
    v
grafana dashboard (browser via Caddy or direct 127.0.0.1:3100)
```

### 3. n8n Health Monitoring Flow

```
n8n Container Health workflow (every 2 min)
    |
    | Already monitors all Docker containers via container-health:9997
    | Sentinel containers auto-detected (healthy/unhealthy/stopped)
    v
Discord webhook alert if sentinel-gateway or sentinel-postgres unhealthy

PLUS (new):
n8n Sentinel Health workflow (every 2 min)
    |
    | HTTP GET http://sentinel-gateway:9201/health
    | HTTP GET http://sentinel-gateway:9201/ready
    v
Discord alert if:
  - /health returns non-200 (gateway process down)
  - /ready returns 503 (all backends unhealthy)
```

**Why both?** Container Health catches "container crashed" (Docker-level). Sentinel Health catches "gateway running but backends broken" (application-level). Different failure modes.

### 4. Cutover Sequence

```
Step 1: Deploy Sentinel stack alongside ContextForge
        sentinel-gateway on 127.0.0.1:9201 (health only, NOT 9200)
        sentinel-postgres on internal port
        Verify: curl http://127.0.0.1:9201/health

Step 2: Verify tool routing
        Manual test: send MCP requests to Sentinel via temp port
        Confirm all 19+ tools respond correctly

Step 3: Stop ContextForge
        docker compose -f docker-compose.yml -f docker-compose.slim.yml down
        (in /home/lwb3/mcp-context-forge/)

Step 4: Rebind Sentinel to port 9200
        Update docker-compose.yml: 127.0.0.1:9200:9200
        docker compose up -d

Step 5: Update Claude Code config
        ~/.claude/settings.json: point sentinel-gateway MCP entry
        to new container/command

Step 6: Smoke test
        Use Claude Code to call tools through Sentinel
        Verify audit logs in sentinel-postgres

Step 7: Deploy monitoring
        Start prometheus + grafana containers
        Verify metrics scraping
        Import dashboard
```

## Docker Network Topology (post-cutover)

```
                    Internet
                       |
                    Caddy (:80, :443)
                       |
            +----------+----------+
            |          |          |
         webproxy   webproxy   webproxy
            |          |          |
       wallybrain   solitaire  authelia
       (:8800)      (:8801)    (:9091)
                       |
                    n8nnet
                       |
            +----------+----------+----------+
            |          |          |          |
          n8n      mcp-n8n    monitoring   sentinel-gateway
         (:5678)   (:3000)   (9997-9999)  (needs n8nnet for
                       |                   n8n health workflow
                  sentinelnet              to reach it)
                       |
            +----------+----------+----------+
            |          |          |          |
     sentinel-gw   sentinel-pg  prometheus  grafana
     (:9200,:9201) (:5432)      (:9090)     (:3100)
                       |
                    mcp-sqlite
                    (:3000)
```

**Key networking decisions:**

1. **`sentinelnet`** -- new Docker network for Sentinel stack. Replaces `mcpnet`.
2. **`mcp-n8n` bridges `sentinelnet` + `n8nnet`** -- same bridge pattern as before, just new network name.
3. **`sentinel-gateway` joins `n8nnet`** -- so n8n health workflows can HTTP GET the health endpoint.
4. **Prometheus on `sentinelnet` only** -- only needs to reach `sentinel-gateway:9201/metrics`.
5. **Grafana on `sentinelnet`** -- queries Prometheus internally. Exposed at `127.0.0.1:3100` for browser access.

## Docker Compose Structure

Single `docker-compose.yml` in `/home/lwb3/sentinel-gateway/`:

```yaml
services:
  gateway:
    build: .
    container_name: sentinel-gateway
    restart: unless-stopped
    ports:
      - "127.0.0.1:9200:9200"   # MCP transport
      - "127.0.0.1:9201:9201"   # Health + metrics
    networks:
      - sentinelnet
      - n8nnet    # so n8n health workflows can reach /health
    environment:
      - JWT_SECRET_KEY=${JWT_SECRET_KEY}
      - DATABASE_URL=postgres://sentinel:${POSTGRES_PASSWORD}@postgres:5432/sentinel
      - RUST_LOG=info
      # stdio backend API keys
      - FIRECRAWL_API_KEY=${FIRECRAWL_API_KEY}
      - EXA_API_KEY=${EXA_API_KEY}
    depends_on:
      postgres:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://127.0.0.1:9201/health"]
      interval: 10s
      timeout: 3s
      retries: 3
      start_period: 5s
    deploy:
      resources:
        limits:
          memory: 256M

  postgres:
    image: postgres:16-alpine
    container_name: sentinel-postgres
    restart: unless-stopped
    networks:
      - sentinelnet
    environment:
      - POSTGRES_USER=sentinel
      - POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
      - POSTGRES_DB=sentinel
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U sentinel -d sentinel"]
      interval: 5s
      timeout: 3s
      retries: 5
      start_period: 10s
    deploy:
      resources:
        limits:
          memory: 256M

  mcp-n8n:
    # existing service, moved from mcpnet
    container_name: mcp-n8n
    networks:
      - sentinelnet
      - n8nnet

  mcp-sqlite:
    # existing service, moved from mcpnet
    container_name: mcp-sqlite
    networks:
      - sentinelnet

  prometheus:
    image: prom/prometheus:latest
    container_name: sentinel-prometheus
    restart: unless-stopped
    networks:
      - sentinelnet
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - promdata:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.retention.time=15d'
      - '--storage.tsdb.retention.size=500MB'
    deploy:
      resources:
        limits:
          memory: 256M

  grafana:
    image: grafana/grafana-oss:latest
    container_name: sentinel-grafana
    restart: unless-stopped
    networks:
      - sentinelnet
    ports:
      - "127.0.0.1:3100:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=${GRAFANA_ADMIN_PASSWORD}
      - GF_SECURITY_ADMIN_USER=admin
      - GF_AUTH_ANONYMOUS_ENABLED=false
    volumes:
      - grafanadata:/var/lib/grafana
      - ./grafana/provisioning:/etc/grafana/provisioning:ro
      - ./grafana/dashboards:/var/lib/grafana/dashboards:ro
    deploy:
      resources:
        limits:
          memory: 128M

volumes:
  pgdata:
  promdata:
  grafanadata:

networks:
  sentinelnet:
    driver: bridge
  n8nnet:
    external: true
```

## Prometheus Configuration

File: `prometheus.yml` in `/home/lwb3/sentinel-gateway/`

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'sentinel-gateway'
    static_configs:
      - targets: ['sentinel-gateway:9201']
    metrics_path: /metrics
    scrape_interval: 15s
```

Single scrape target. No service discovery needed -- there is exactly one gateway instance.

## Grafana Dashboard Panels

Provision via JSON file at `grafana/dashboards/sentinel.json`. Key panels:

| Panel | Query | Type |
|-------|-------|------|
| Request Rate | `rate(sentinel_requests_total[5m])` | Time series |
| Request Rate by Tool | `sum by (tool)(rate(sentinel_requests_total[5m]))` | Time series |
| Error Rate | `rate(sentinel_errors_total[5m])` | Time series |
| P50/P95/P99 Latency | `histogram_quantile(0.95, rate(sentinel_request_duration_seconds_bucket[5m]))` | Time series |
| Backend Health | `sentinel_backend_healthy` | Stat (green/red) |
| Rate Limit Hits | `rate(sentinel_rate_limit_hits_total[5m])` | Time series |
| Requests by Status | `sum by (status)(sentinel_requests_total)` | Pie chart |

Provision the Prometheus datasource automatically:

```yaml
# grafana/provisioning/datasources/prometheus.yml
apiVersion: 1
datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
```

```yaml
# grafana/provisioning/dashboards/default.yml
apiVersion: 1
providers:
  - name: 'default'
    folder: ''
    type: file
    options:
      path: /var/lib/grafana/dashboards
```

## n8n Health Workflow Integration

### Option A: Extend existing Container Health workflow (recommended)

The existing container-health endpoint at `:9997` already reports all Docker containers. When Sentinel containers start, they automatically appear in the health report. The existing "Container Health" n8n workflow (every 2 min) will catch stopped/unhealthy Sentinel containers with no changes.

### Option B: Add application-level health check (also do this)

New n8n workflow: "Sentinel Gateway Health"

```
Trigger: Cron every 2 minutes
  |
  v
HTTP Request: GET http://sentinel-gateway:9201/health
  |
  +--[non-200 or timeout]---> Discord webhook: "Sentinel Gateway DOWN"
  |
  v
HTTP Request: GET http://sentinel-gateway:9201/ready
  |
  +--[503]---> Discord webhook: "Sentinel: all backends unhealthy"
  |
  v
(success: do nothing)
```

**Prerequisite:** sentinel-gateway must be on `n8nnet` so n8n can reach it. Already handled in the docker-compose above.

## Claude Code MCP Configuration Change

### Before (ContextForge)

```json
{
  "mcpServers": {
    "sentinel-gateway": {
      "command": "docker",
      "args": ["run", "--rm", "-i", "--network=host", "...wrapper..."],
      "env": { "MCP_AUTH": "Bearer <jwt>" }
    },
    "context7": { "command": "npx", "args": ["@upstash/context7-mcp"] },
    "firecrawl": { "command": "npx", "args": ["firecrawl-mcp"] },
    "exa": { "command": "npx", "args": ["exa-mcp"] },
    "playwright": { "command": "npx", "args": ["@playwright/mcp"] },
    "sequential-thinking": { "command": "npx", "args": ["@modelcontextprotocol/server-sequential-thinking"] }
  }
}
```

### After (Sentinel)

```json
{
  "mcpServers": {
    "sentinel-gateway": {
      "command": "docker",
      "args": [
        "run", "--rm", "-i",
        "--network=host",
        "sentinel-gateway",
        "--config", "/etc/sentinel/sentinel.toml"
      ],
      "env": {
        "JWT_SECRET_KEY": "<from .env>",
        "DATABASE_URL": "postgres://sentinel:<pw>@127.0.0.1:5432/sentinel"
      }
    }
  }
}
```

**Key change:** All 5 ungoverned stdio servers are removed from `mcpServers`. They are now managed internally by Sentinel as stdio backends. Single entry point.

**Alternative:** If Claude Code connects via HTTP (wrapper pattern) rather than direct stdio, the wrapper container stays but points to `http://localhost:9200` (same port, now Sentinel instead of ContextForge).

## Port Summary

| Port | Service | Binding | New? |
|------|---------|---------|------|
| 9200 | Sentinel MCP transport | `127.0.0.1` | Reuses ContextForge port |
| 9201 | Sentinel health/metrics | `127.0.0.1` | New (was temporary during dev) |
| 9090 | Prometheus | Docker internal | New |
| 3100 | Grafana | `127.0.0.1` | New |
| 5432 | sentinel-postgres | Docker internal | Replaces ContextForge postgres (was 5434) |

## File Structure for Deployment

```
/home/lwb3/sentinel-gateway/
  docker-compose.yml          # Updated with all services
  Dockerfile                  # Already exists (multi-stage Rust build)
  sentinel.toml               # Already exists (gateway config)
  .env                        # Secrets (JWT_SECRET_KEY, POSTGRES_PASSWORD, etc.)
  prometheus.yml              # New: Prometheus scrape config
  grafana/
    provisioning/
      datasources/
        prometheus.yml        # New: auto-provision Prometheus datasource
      dashboards/
        default.yml           # New: dashboard provider config
    dashboards/
      sentinel.json           # New: Grafana dashboard JSON
```

## Build Order (dependency-driven)

```
Phase 1: Deploy Sentinel alongside ContextForge
  - Update docker-compose.yml (add networks, resource limits)
  - Create sentinelnet, connect to n8nnet
  - Build and start sentinel-gateway + sentinel-postgres
  - Verify /health and /metrics on :9201
  Dependencies: None

Phase 2: Cutover from ContextForge
  - Stop ContextForge stack
  - Rebind Sentinel to port 9200
  - Update ~/.claude/settings.json
  - Remove ungoverned MCP server entries
  - Smoke test all tools
  Dependencies: Phase 1

Phase 3: Network hardening
  - Verify 127.0.0.1 binding on all Sentinel ports
  - Verify iptables rules (no new external exposure)
  - Remove mcpnet Docker network
  Dependencies: Phase 2

Phase 4: Prometheus + Grafana
  - Add prometheus + grafana services to docker-compose.yml
  - Create prometheus.yml, grafana provisioning files
  - Create Grafana dashboard JSON
  - Verify metrics scraping
  Dependencies: Phase 1 (can run parallel with Phase 2-3)

Phase 5: n8n health monitoring
  - Create "Sentinel Gateway Health" n8n workflow
  - Test Discord alerts for /health failure
  - Test Discord alerts for /ready failure (all backends down)
  Dependencies: Phase 2 (needs Sentinel on production port)
```

**Critical path:** Phases 1 -> 2 -> 3 are sequential (deployment order matters). Phase 4 can start after Phase 1. Phase 5 needs Phase 2 complete.

## Scalability Considerations

| Concern | At current scale (1 user) | At 10 users | At 100 users |
|---------|--------------------------|-------------|--------------|
| Prometheus storage | ~10 MB/month (5 metrics, 15s interval) | Same (metrics are per-gateway, not per-user) | Same per instance |
| Grafana | Single dashboard, negligible | Same | Same |
| Health checks | 1 n8n workflow, trivial | Same | Same |
| Gateway RAM | ~50 MB | ~60 MB (more concurrent requests) | Multiple instances behind LB |

## Sources

- Existing Sentinel codebase: `src/metrics/mod.rs`, `src/health/server.rs`
- Existing infrastructure: `docs/CURRENT-INFRASTRUCTURE.md`, `docs/MCP-TOPOLOGY.md`
- Sentinel config: `sentinel.toml`, `docker-compose.yml`, `Dockerfile`
- ContextForge deployment: `docs/CONTEXTFORGE-GATEWAY.md`, `/home/lwb3/mcp-context-forge/docker-compose.slim.yml`
- [Prometheus Docker deployment](https://prometheus.io/docs/prometheus/latest/installation/)
- [Grafana Docker deployment](https://grafana.com/docs/grafana/latest/setup-grafana/installation/docker/)
- [Grafana provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/)
