# Feature Research

**Domain:** Production deployment, monitoring, and hardening for an MCP gateway (Rust/Docker)
**Researched:** 2026-02-22
**Confidence:** HIGH

**Scope note:** This research covers v1.1 (Deploy & Harden) features only. For v1.0 gateway features (auth, RBAC, routing, audit, etc.), see git history of this file.

## Feature Landscape

### Table Stakes (Users Expect These)

Features that are non-negotiable for a production deployment cutover. Missing any of these means the deployment is incomplete or fragile.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Clean ContextForge-to-Sentinel cutover | Entire purpose of v1.1 -- replace predecessor | LOW | Stop ContextForge containers, start Sentinel, update MCP config. Sentinel already listens on 9200 per `sentinel.toml`. Same port means sequential swap, not parallel. ~5s downtime, acceptable for single user. |
| Claude MCP config update | Claude Code must route through Sentinel not ContextForge | LOW | Update `~/.claude/settings.json` MCP server entry. Point at `127.0.0.1:9200`. Generate new JWT for Sentinel auth. Token currently at `/tmp/claude-1001/gateway-token.txt`. |
| End-to-end tool verification | Must confirm all 7 backends work through Sentinel | LOW | Call one tool per backend (n8n, sqlite, context7, firecrawl, exa, playwright, sequential-thinking), verify JSON-RPC response matches ContextForge behavior. 138 tests passed in dev but need live verification with real backends and real API keys. |
| 127.0.0.1 binding verification | Gateway must not be reachable from public internet | LOW | Already configured: `sentinel.toml` has `listen = "127.0.0.1:9200"`, `docker-compose.yml` maps `127.0.0.1:9201:9201`. Verify with `curl` from external IP returns connection refused. |
| iptables rules for Sentinel ports | Defense-in-depth even though bound to localhost | LOW | Match existing VPS pattern: `iptables -A INPUT -i eth0 -p tcp --dport 9200 -j DROP` and same for 9201. Add to `/home/lwb3/v1be-code-server/fix-iptables.sh`. |
| Docker health checks | Container orchestration must detect if Sentinel is alive | ALREADY DONE | `docker-compose.yml` already has health check curling `/health` on 9201. Postgres has `pg_isready`. Both verified in v1.0. |
| Prometheus scrape config | Prometheus must pull metrics from `/metrics` endpoint | LOW | Add `prometheus.yml` job targeting `sentinel-gateway:9201`. Sentinel exposes 5 metric families: `sentinel_requests_total`, `sentinel_request_duration_seconds`, `sentinel_errors_total`, `sentinel_backend_healthy`, `sentinel_rate_limit_hits_total`. |
| Grafana dashboard for Sentinel metrics | Visibility into gateway operations | MEDIUM | JSON dashboard provisioned via `/etc/grafana/provisioning/dashboards/`. Five panels: request rate (by tool/status), latency percentiles (p50/p95/p99), error rate, backend health gauge, rate limit hits. |
| Grafana datasource provisioning | Grafana must auto-connect to Prometheus on startup | LOW | YAML in `/etc/grafana/provisioning/datasources/` pointing at `http://prometheus:9090`. Standard pattern, well-documented. |
| n8n health monitoring with Discord alerts | Automated alerting when Sentinel goes down | MEDIUM | n8n Schedule trigger every 5 min, HTTP Request to `http://sentinel-gateway:9201/health`. On non-200 or timeout, Discord webhook with red embed. Existing pattern proven: VPS has heartbeat workflow `zC3ZEX1gtzZr3m62` to clone from. |
| Discord alert on failure | Operator must be notified when gateway is unhealthy | LOW | Discord webhook POST with embed. Already using webhooks for daily health heartbeat and security reminders. Webhook URL already configured in n8n. |
| Rollback plan documented | Must be able to revert to ContextForge if Sentinel fails | LOW | Steps: stop Sentinel containers, restart ContextForge (`docker compose -f docker-compose.yml -f docker-compose.slim.yml up -d`), revert MCP config. ContextForge images and pgdata volume preserved during cutover. |

### Differentiators (Competitive Advantage)

Features beyond table stakes that add meaningful operational value.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Grafana alert rules with Discord | Catches metric anomalies that n8n health check misses (error rate spike while `/health` still returns 200) | MEDIUM | Grafana alerting with Discord contact point. Rules: error rate > 10% over 5 min, p99 latency > 30s, any backend unhealthy > 2 min. Independent alert path from n8n. |
| Audit log rotation | Prevent unbounded Postgres disk growth from audit logs | LOW | Cron job or n8n workflow: `DELETE FROM audit_logs WHERE created_at < NOW() - INTERVAL '30 days'`. Alternatively Postgres table partitioning by month. |
| VPS reboot restart verification | Confirm containers survive reboot | LOW | `restart: unless-stopped` already set. Need to test once after cutover is stable. |
| Sentinel resource tracking (cAdvisor) | Track if Sentinel stays under 100 MB RAM target over time | LOW | cAdvisor container or Docker stats exporter alongside Prometheus. Shows RSS, CPU usage trends. |
| Nightly Postgres backup for Sentinel DB | Protect audit log data | LOW | Extend existing `/home/lwb3/backups/nightly-db-backup.sh` to include `pg_dump` of sentinel database, or backup the pgdata Docker volume. |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Blue/green deployment | Zero-downtime cutover | Overkill for single-user localhost gateway. Needs reverse proxy, port juggling, permanent infrastructure. Downtime is ~5s affecting one user. Already in project out-of-scope. | Sequential swap: stop old, start new. Keep ContextForge images for rollback. |
| Prometheus Alertmanager | Standard Prometheus alerting | Another container, config file, routing tree, deduplication. Single operator with Discord does not need this. | Grafana alert rules with Discord contact point + n8n health check. Two independent paths without Alertmanager overhead. |
| Loki log aggregation | Centralized log search | Sentinel logs structured JSON to stdout (Docker captures). `docker logs` + Postgres audit covers needs. Loki adds 500+ MB RAM for marginal benefit. | `docker logs --since 1h sentinel-gateway` for debugging. Postgres audit for tool call history. |
| OpenTelemetry tracing | Distributed tracing | Single-hop architecture (gateway to backend). No multi-service correlation needed. Already out-of-scope. Adds 2-3 containers. | Prometheus metrics for latency. Postgres audit for per-request detail. Request IDs in logs. |
| Automated rollback on failure | Self-healing if Sentinel fails | Dangerous: incorrect health check = thrashing between services. ContextForge and Sentinel have different configs/secrets. Automatic switching could corrupt state. | Manual rollback with documented steps. Discord alert provides time to investigate. |
| Nginx reverse proxy | Standard production pattern | Sentinel is localhost-only, one client (Claude Code via stdio). No TLS, no load balancing, no caching benefit. Adds latency and failure point. | Direct connection. Axum handles the single client. |
| 20+ panel monitoring dashboard | Complete observability | 5 metric families means 5 panels. More panels = noise. Focus on actionable panels. | Focused: request rate, error rate, latency histogram, backend health, rate limit hits. |
| Parallel run (Sentinel + ContextForge) | Compare behavior side-by-side | Both bind to 9200. Would need port change, dual config, dual JWT. 138 tests + 47 requirements already verified compatibility. | Sequential cutover with rollback plan. Verify tools end-to-end after swap. |

## Feature Dependencies

```
[Sentinel containers up + healthy]
    |
    +--requires--> [docker-compose.yml with .env secrets]
    +--requires--> [Postgres healthy + migrations applied]
    |
    v
[ContextForge shutdown]
    +--requires--> [Sentinel containers verified healthy]
    |
    v
[Claude MCP config update + JWT]
    +--requires--> [Sentinel listening on 9200]
    |
    v
[End-to-end tool verification]
    +--requires--> [Claude MCP config pointing at Sentinel]
    +--requires--> [All 7 backends reachable from Sentinel container]
    |
    v
[CUTOVER COMPLETE -- monitoring can begin]
    |
    +------+------+------+
    |      |      |      |
    v      v      v      v
[Prom] [Grafana] [n8n]  [iptables]
   |      |       |
   v      v       v
[scrape] [dash]  [Discord alert]
   |      |
   +--+---+
      |
      v
   [Grafana alerting]
```

### Dependency Notes

- **Cutover is the critical path.** Everything else depends on Sentinel running. Do cutover first, layer monitoring on top.
- **Monitoring has two independent tracks.** Prometheus/Grafana (metrics visualization) and n8n (health polling + Discord). Can be built in parallel after cutover.
- **iptables is independent.** Can be hardened at any point. Good to do early to prevent accidental exposure.
- **Grafana alerting depends on both Prometheus and Grafana.** Add after dashboard confirms metrics flow correctly.
- **n8n health check is the fastest path to alerting.** Does not need Prometheus/Grafana at all. Can work immediately after cutover.

## MVP Definition

### Launch With (v1.1 core -- cutover + minimum monitoring)

- [ ] Start Sentinel containers on VPS (`docker compose up -d` with production .env)
- [ ] Stop ContextForge containers (preserve images/volumes for rollback)
- [ ] Update Claude MCP config to point at Sentinel on 9200
- [ ] Generate JWT token for Sentinel auth
- [ ] Verify all 7 backends work end-to-end
- [ ] Verify 127.0.0.1 binding (not reachable from public IP)
- [ ] Add iptables DROP rules for 9200/9201 on eth0, update `fix-iptables.sh`
- [ ] n8n health check workflow: poll `/health` every 5 min, Discord alert on failure
- [ ] Prometheus container scraping Sentinel `/metrics`
- [ ] Grafana container with provisioned datasource and 5-panel dashboard
- [ ] Rollback plan documented

### Add After Validation (v1.1 polish -- after stable for 24h)

- [ ] Grafana alert rules (error rate, latency, backend health) with Discord contact point
- [ ] Audit log rotation (30-day retention)
- [ ] VPS reboot restart verification
- [ ] Extend nightly backup script for Sentinel Postgres

### Future Consideration (v2+)

- [ ] OpenTelemetry tracing -- only if multi-hop routing added
- [ ] Alertmanager -- only if alert routing complexity exceeds Grafana rules
- [ ] Loki -- only if `docker logs` proves insufficient
- [ ] cAdvisor resource monitoring -- only if resource usage becomes a concern

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Clean cutover (stop CF, start Sentinel) | HIGH | LOW | P1 |
| Claude MCP config update + JWT | HIGH | LOW | P1 |
| End-to-end tool verification | HIGH | LOW | P1 |
| 127.0.0.1 binding verification | HIGH | LOW | P1 |
| iptables hardening | HIGH | LOW | P1 |
| Rollback plan documented | HIGH | LOW | P1 |
| n8n health check + Discord alert | HIGH | MEDIUM | P1 |
| Prometheus scrape config | MEDIUM | LOW | P1 |
| Grafana datasource provisioning | MEDIUM | LOW | P1 |
| Grafana dashboard (5 panels) | MEDIUM | MEDIUM | P1 |
| Grafana alert rules + Discord | MEDIUM | MEDIUM | P2 |
| Audit log rotation | MEDIUM | LOW | P2 |
| Nightly Postgres backup | MEDIUM | LOW | P2 |
| VPS reboot restart test | LOW | LOW | P2 |
| cAdvisor resource monitoring | LOW | LOW | P3 |

**Priority key:**
- P1: Must have for v1.1 launch
- P2: Should have, add after P1 stable (24h)
- P3: Nice to have, future consideration

## Existing Infrastructure to Leverage

| Existing Asset | How It Helps | Location |
|----------------|-------------|----------|
| n8n health heartbeat workflow | Clone and modify for Sentinel health check | Workflow `zC3ZEX1gtzZr3m62` |
| Discord webhook | Already configured in n8n for alerts | n8n credentials |
| `fix-iptables.sh` | Add Sentinel ports to existing script | `/home/lwb3/v1be-code-server/fix-iptables.sh` |
| Docker log rotation | Global config, Sentinel inherits automatically | `/etc/docker/daemon.json` (10m, 3 files) |
| Nightly backup script | Extend for Sentinel Postgres | `/home/lwb3/backups/nightly-db-backup.sh` |
| Global pre-commit hook | Prevents committing .env with secrets | `~/.git-hooks/pre-commit` |
| ContextForge monitoring profile | Reference for Prometheus/Grafana compose pattern | `/home/lwb3/mcp-context-forge/docker-compose.yml` |
| JWT token generation | Pattern established for ContextForge | Token at `/tmp/claude-1001/gateway-token.txt` |

## Prometheus Metrics Already Available

Sentinel v1.0 exposes 5 metric families at `/metrics` on port 9201. No additional instrumentation needed.

| Metric | Type | Labels | Dashboard Panel |
|--------|------|--------|-----------------|
| `sentinel_requests_total` | Counter | `tool`, `status` | Request rate by tool, success/error split |
| `sentinel_request_duration_seconds` | Histogram | `tool` | Latency percentiles (p50, p95, p99) |
| `sentinel_errors_total` | Counter | `tool`, `error_type` | Error rate by type |
| `sentinel_backend_healthy` | Gauge | `backend` | Backend health (1=up, 0=down, per backend) |
| `sentinel_rate_limit_hits_total` | Counter | `tool` | Rate limit hit frequency |

## Sources

- [Grafana provisioning documentation](https://grafana.com/docs/grafana/latest/administration/provisioning/)
- [Grafana Prometheus datasource configuration](https://grafana.com/docs/grafana/latest/datasources/prometheus/configure/)
- [Prometheus with Docker Compose setup guide](https://last9.io/blog/prometheus-with-docker-compose/)
- [Grafana + Prometheus Docker Compose tutorial](https://www.doc.ic.ac.uk/~nuric/posts/sysadmin/how-to-setup-grafana-and-prometheus-with-docker-compose/)
- [n8n health monitoring workflow template](https://n8n.io/workflows/8412-website-and-api-health-monitoring-system-with-http-status-validation/)
- [n8n monitoring documentation](https://docs.n8n.io/hosting/logging-monitoring/monitoring/)
- [Docker-rollout zero-downtime deployment](https://github.com/wowu/docker-rollout) (reviewed, decided against -- overkill)
- Sentinel Gateway source: `src/metrics/mod.rs`, `src/health/server.rs`
- ContextForge compose: `/home/lwb3/mcp-context-forge/docker-compose.yml` (monitoring profile reference)
- VPS infrastructure: MEMORY.md (containers, iptables, n8n workflows, backups)

---
*Feature research for: Sentinel Gateway v1.1 Deploy & Harden*
*Researched: 2026-02-22*
