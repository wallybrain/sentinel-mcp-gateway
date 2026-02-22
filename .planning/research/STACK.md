# Technology Stack

**Project:** Sentinel Gateway v1.1 -- Deploy, Monitor & Harden
**Researched:** 2026-02-22
**Scope:** Stack additions for deployment, Prometheus/Grafana monitoring, n8n health checks, Docker networking cutover. Does NOT revisit v1.0 core stack (Rust, axum, tokio, sqlx, etc.).

## Recommended Stack Additions

### Monitoring -- Prometheus + Grafana

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| Prometheus | 3.5.1 LTS | Metrics scraping | Current LTS. Scrapes Sentinel's existing `/metrics` endpoint (already exposes `sentinel_requests_total`, `sentinel_request_duration_seconds`, `sentinel_errors_total`, `sentinel_backend_healthy`, `sentinel_rate_limit_hits_total`). No code changes needed. | HIGH |
| Grafana | 12.3.x | Dashboard & alerting | Current stable. Provisioning via YAML for datasources + dashboards. Anonymous read-only access behind Caddy/Authelia. | HIGH |

**Why standalone Prometheus+Grafana, not ContextForge's monitoring profile:**
ContextForge bundles Prometheus+Grafana+Loki+Tempo+cAdvisor+postgres_exporter+pgbouncer_exporter+nginx_exporter in its `--profile monitoring`. That's 8 containers for a full observability stack designed for ContextForge's multi-replica architecture. Sentinel is a single binary with 5 built-in metrics. Two containers (Prometheus + Grafana) are sufficient. Adding the extras would waste ~500 MB RAM on a VPS already running 14 containers.

**What NOT to add:**
- Loki/Promtail -- log aggregation is overkill. `docker logs sentinel-gateway` + Postgres audit logs cover logging needs.
- Tempo -- distributed tracing explicitly deferred to v2 (see PROJECT.md Out of Scope: OpenTelemetry).
- cAdvisor -- container metrics are nice-to-have but the existing n8n container-health workflow already monitors container status. Defer.
- postgres_exporter -- Sentinel's Postgres is lightweight (audit logs only, <100 MB). Not worth a dedicated exporter.
- AlertManager -- Grafana has built-in alerting since v9. Using Grafana alerting with Discord webhook contact point avoids an extra container.

### n8n Health Monitoring

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| n8n (existing) | latest (already running) | Health check workflows | Already running at `127.0.0.1:8567` with 4 monitoring workflows (CPU, disk, memory, container-health). Add a Sentinel-specific workflow. | HIGH |
| Discord Webhook (existing) | -- | Alert destination | Already configured for all 4 existing monitoring workflows. Same webhook. | HIGH |

**New n8n workflow needed:** "Sentinel Gateway Health Check"
- Trigger: Cron every 2 minutes
- Action: HTTP GET `http://host.docker.internal:9201/health` (n8n uses `extra_hosts: host.docker.internal:host-gateway`)
- On failure: Discord embed with gateway status, timestamp, consecutive failure count
- On recovery: Discord embed confirming recovery

**Why n8n, not Grafana alerting for health checks:**
Both work. Use n8n for binary health (up/down) because it matches the existing container-health pattern and sends richer Discord embeds. Use Grafana alerting for metric thresholds (high latency, error rates, backend failures) because those require PromQL queries.

### Docker Networking for Cutover

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| Docker Compose | v2 (already installed) | Service orchestration | Sentinel already has a `docker-compose.yml`. Extend with network config. | HIGH |

**Current Docker network topology (verified):**

| Network | Containers | Purpose |
|---------|------------|---------|
| `mcp-context-forge_mcpnet` | mcp-context-forge-gateway-1, mcp-context-forge-postgres-1, mcp-context-forge-redis-1, mcp-n8n, mcp-sqlite | ContextForge + MCP backends |
| `n8n-mcp_default` | n8n, system-stats, container-health, chiasm, mcp-n8n, cpu-server | n8n monitoring stack |
| `webproxy` | caddy, solitaire, chiasm, authelia, wallybrain-music | Public web services |

**Critical observation:** `mcp-n8n` is on BOTH `mcp-context-forge_mcpnet` and `n8n-mcp_default`. This is because it's defined in ContextForge's compose but shares the n8n network for communication. The MCP backend containers (mcp-n8n, mcp-sqlite) need to be accessible to Sentinel after cutover.

**Cutover strategy -- join existing network, not create new one:**

```yaml
# sentinel-gateway/docker-compose.yml additions
services:
  gateway:
    networks:
      - sentinel-net        # Private: gateway <-> sentinel-postgres
      - mcp-context-forge_mcpnet  # Shared: gateway <-> mcp-n8n, mcp-sqlite

  postgres:
    networks:
      - sentinel-net        # Private: only gateway needs DB access

networks:
  sentinel-net:
    driver: bridge
  mcp-context-forge_mcpnet:
    external: true          # Join ContextForge's existing network
```

**Why join `mcp-context-forge_mcpnet` instead of creating a new network:**
- `mcp-n8n` and `mcp-sqlite` are already on this network with DNS names `mcp-n8n` and `mcp-sqlite`.
- Sentinel's `sentinel.toml` already references `http://mcp-n8n:3000` and `http://mcp-sqlite:3000`.
- Creating a new network would require either moving the MCP backend containers or using host networking, both more disruptive.
- After ContextForge is stopped, the network persists (Docker keeps external networks alive).

**Cutover sequence:**
1. Start Sentinel on `127.0.0.1:9201` (already configured, different port from ContextForge's 9200)
2. Verify all 7 backends respond through Sentinel
3. Update `~/.claude/settings.json` to point MCP at port 9201
4. Verify Claude Code tools work
5. Stop ContextForge (`docker compose down` in `/home/lwb3/mcp-context-forge/`)
6. Optionally rebind Sentinel to port 9200 for consistency

**Port decision:** Keep 9201 or switch to 9200. Recommendation: switch to 9200 after ContextForge is down, because `~/.claude/settings.json` currently points at 9200 and changing it requires a Claude Code restart. Switching Sentinel to 9200 means updating one line in `sentinel.toml` and `docker-compose.yml`, then `docker compose up -d`.

### Network Hardening

No new tools needed. Use existing iptables + Docker bind address.

| Concern | Current State | Action Needed |
|---------|--------------|---------------|
| Gateway bind address | `127.0.0.1:9201` in docker-compose.yml | Already correct. Verify no `0.0.0.0` binding. |
| Postgres bind address | No port mapping (internal only) | Already correct. Only gateway container can reach it via Docker network. |
| Prometheus scrape | Not yet deployed | Bind to `127.0.0.1:9090`. Scrapes gateway on Docker internal network. |
| Grafana UI | Not yet deployed | Bind to `127.0.0.1:3000`. Caddy reverse proxy with Authelia for public access. |
| iptables | Existing rules block eth0 for 8080, 9999 | No new rules needed if Prometheus/Grafana bind to 127.0.0.1. |

## Prometheus Configuration

```yaml
# sentinel-gateway/monitoring/prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'sentinel-gateway'
    static_configs:
      - targets: ['sentinel-gateway:9201']
    metrics_path: '/metrics'
    scrape_interval: 10s
```

**Why 10s scrape interval for Sentinel:** MCP tool calls are bursty (Claude sends multiple tools in rapid succession). 15s default could miss short-lived spikes. 10s gives better resolution without meaningfully increasing storage (~40 bytes/sample, negligible).

## Grafana Configuration

### Provisioned Datasource

```yaml
# sentinel-gateway/monitoring/grafana/provisioning/datasources/prometheus.yml
apiVersion: 1
datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
```

### Dashboard Panels (pre-provisioned)

| Panel | PromQL | Purpose |
|-------|--------|---------|
| Request Rate | `rate(sentinel_requests_total[5m])` | Requests per second by tool and status |
| Error Rate | `rate(sentinel_errors_total[5m])` | Errors per second by type |
| Latency P50/P95/P99 | `histogram_quantile(0.95, rate(sentinel_request_duration_seconds_bucket[5m]))` | Request latency distribution |
| Backend Health | `sentinel_backend_healthy` | Up/down status per backend |
| Rate Limit Hits | `rate(sentinel_rate_limit_hits_total[5m])` | Rate limiting activity |
| Gateway Up | `up{job="sentinel-gateway"}` | Prometheus can reach the gateway |

### Grafana Alerting (Discord)

| Alert | Condition | Severity |
|-------|-----------|----------|
| Gateway Down | `up{job="sentinel-gateway"} == 0` for 1m | Critical |
| Backend Unhealthy | `sentinel_backend_healthy == 0` for 2m | Warning |
| High Error Rate | `rate(sentinel_errors_total[5m]) > 0.5` | Warning |
| High Latency | `histogram_quantile(0.95, ...) > 5` for 5m | Warning |

Contact point: Discord webhook (same URL as existing n8n alerts).

## Docker Compose Additions

```yaml
# Added to sentinel-gateway/docker-compose.yml
services:
  prometheus:
    image: prom/prometheus:v3.5.1
    container_name: sentinel-prometheus
    restart: unless-stopped
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheusdata:/prometheus
    ports:
      - "127.0.0.1:9090:9090"
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.retention.time=30d'
      - '--storage.tsdb.retention.size=1GB'
    networks:
      - sentinel-net

  grafana:
    image: grafana/grafana:12.3.1
    container_name: sentinel-grafana
    restart: unless-stopped
    environment:
      - GF_SECURITY_ADMIN_USER=${GRAFANA_ADMIN_USER:-admin}
      - GF_SECURITY_ADMIN_PASSWORD=${GRAFANA_ADMIN_PASSWORD}
      - GF_SERVER_ROOT_URL=https://grafana.wallybrain.icu
      - GF_AUTH_ANONYMOUS_ENABLED=false
    volumes:
      - grafanadata:/var/lib/grafana
      - ./monitoring/grafana/provisioning:/etc/grafana/provisioning:ro
    ports:
      - "127.0.0.1:3000:3000"
    depends_on:
      - prometheus
    networks:
      - sentinel-net

volumes:
  prometheusdata:
  grafanadata:
```

**Storage limits:** `--storage.tsdb.retention.time=30d` and `--storage.tsdb.retention.size=1GB` prevent Prometheus from consuming disk. With 5 metrics scraped every 10s, 30 days of data is roughly 50-100 MB. The 1GB cap is a safety net.

**Grafana access:** Bind to `127.0.0.1:3000`, proxy through Caddy at `grafana.wallybrain.icu` (or a subpath) with Authelia 2FA. The `GF_AUTH_ANONYMOUS_ENABLED=false` ensures Grafana itself also requires login.

## Resource Impact

| Container | Expected RAM | CPU | Disk |
|-----------|-------------|-----|------|
| Prometheus | ~50-80 MB | Minimal | ~50-100 MB (30d retention) |
| Grafana | ~50-80 MB | Minimal | ~10 MB (dashboards + sqlite) |
| **Total new** | **~100-160 MB** | **Negligible** | **~60-110 MB** |

VPS has 16 GB RAM with 14 containers currently. Adding ~150 MB is well within headroom.

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Metrics scraping | Prometheus standalone | ContextForge monitoring profile | 8 containers vs 1; overkill for 5 metrics from single service |
| Dashboards | Grafana | Prometheus built-in UI | Prometheus UI shows raw PromQL, no persistent dashboards or alerting |
| Log aggregation | Docker logs + Postgres audit | Loki + Promtail | Extra 2 containers; `docker logs` + audit table sufficient for single-instance |
| Health alerts (binary) | n8n workflow | Grafana alerting only | n8n matches existing monitoring pattern; richer Discord embeds |
| Metric alerts (threshold) | Grafana alerting | AlertManager | Grafana built-in alerting avoids extra container; sufficient for single-instance |
| Container metrics | Skip for now | cAdvisor | Existing container-health n8n workflow covers up/down; cAdvisor adds ~100MB RAM |
| Network cutover | Join existing mcpnet | Create new network | MCP backends already have DNS on mcpnet; less disruptive |

## What NOT to Add (Explicit)

| Technology | Why Skip |
|------------|----------|
| Loki/Promtail | Log aggregation deferred. Docker logs + Postgres audit logs are queryable and sufficient. |
| Tempo | Distributed tracing explicitly out of scope (PROJECT.md). |
| cAdvisor | Container metrics not needed when container-health workflow already monitors status. |
| postgres_exporter | Sentinel Postgres is lightweight audit-only. Not worth a dedicated exporter. |
| AlertManager | Grafana built-in alerting handles Discord webhooks directly. |
| Redis | No caching or shared state needed for single-instance. |
| Traefik/Nginx | Caddy already handles reverse proxy + TLS for all VPS services. |
| OpenTelemetry SDK | Explicitly deferred to v2. Prometheus crate already in Rust binary. |

## Installation

No Rust code changes needed. The `/metrics` endpoint already exists.

```bash
# Directory structure to create
mkdir -p monitoring/grafana/provisioning/{datasources,dashboards}

# Files to create:
# monitoring/prometheus.yml
# monitoring/grafana/provisioning/datasources/prometheus.yml
# monitoring/grafana/provisioning/dashboards/dashboard.yml (provider config)
# monitoring/grafana/provisioning/dashboards/sentinel.json (dashboard JSON)
```

## Sources

- [Prometheus 3.5.1 LTS release](https://github.com/prometheus/prometheus/releases) -- current stable, Jan 2026
- [Grafana 12.3 release](https://grafana.com/docs/grafana/latest/whatsnew/whats-new-in-v12-3/) -- current stable, Feb 2026
- [Grafana alerting with Discord](https://grafana.com/docs/grafana/latest/alerting/configure-notifications/manage-contact-points/) -- built-in contact points
- [Prometheus storage sizing](https://prometheus.io/docs/prometheus/latest/storage/) -- ~1-2 bytes per sample
- Sentinel Gateway `/metrics` endpoint -- 5 metric families, standard Prometheus text format
- VPS container audit (2026-02-22) -- 14 running containers, 16 GB RAM, 34% disk used
