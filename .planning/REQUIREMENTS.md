# Requirements: Sentinel Gateway v1.1

**Defined:** 2026-02-22
**Core Value:** Every MCP tool call passes through one governed point with auth, audit, and rate limiting

## v1.1 Requirements

Requirements for Deploy & Harden milestone. Each maps to roadmap phases.

### Pre-Cutover Preparation

- [ ] **PREP-01**: Port config is consistent across sentinel.toml, Dockerfile, and docker-compose.yml (resolve 9200/9201 drift)
- [ ] **PREP-02**: Sidecar service definitions (mcp-n8n, mcp-sqlite) are migrated from ContextForge compose to Sentinel compose
- [ ] **PREP-03**: Sentinel containers join a Docker network that can reach HTTP backends (mcp-n8n, mcp-sqlite) by hostname
- [ ] **PREP-04**: Production .env file exists with JWT secret and Postgres credentials (not committed)
- [ ] **PREP-05**: Sentinel containers start and pass health checks alongside running ContextForge

### Cutover

- [ ] **CUT-01**: ContextForge gateway process is stopped (containers preserved for rollback, not `docker compose down`)
- [ ] **CUT-02**: Sentinel is listening on port 9200 for MCP traffic and 9201 for health/metrics
- [ ] **CUT-03**: Claude Code MCP config updated with new Sentinel entry and fresh JWT token
- [ ] **CUT-04**: All 7 backends respond to tool calls through Sentinel (n8n, sqlite, context7, firecrawl, exa, playwright, sequential-thinking)
- [ ] **CUT-05**: Rollback procedure is documented and tested (restart ContextForge, revert MCP config)

### Network Hardening

- [ ] **NET-01**: Sentinel ports (9200, 9201) are bound to 127.0.0.1 only (verified unreachable from public IP)
- [ ] **NET-02**: iptables DROP rules exist for ports 9200 and 9201 on eth0
- [ ] **NET-03**: fix-iptables.sh is updated with Sentinel port rules
- [ ] **NET-04**: Stale Docker networks from ContextForge are cleaned up

### Monitoring

- [ ] **MON-01**: Prometheus container scrapes Sentinel /metrics endpoint on a regular interval
- [ ] **MON-02**: Grafana container starts with provisioned Prometheus datasource (auto-configured, no manual setup)
- [ ] **MON-03**: Grafana dashboard displays 5 panels: request rate, error rate, latency percentiles, backend health, rate limit hits
- [ ] **MON-04**: n8n workflow polls Sentinel /health endpoint every 2-5 minutes
- [ ] **MON-05**: Discord alert fires when Sentinel health check fails
- [ ] **MON-06**: Discord alert fires when Sentinel health check recovers (back to healthy)

### Operations

- [ ] **OPS-01**: Audit log rotation deletes records older than 30 days (cron or n8n workflow)
- [ ] **OPS-02**: Nightly backup script includes Sentinel Postgres database
- [ ] **OPS-03**: Sentinel containers restart automatically after VPS reboot (verified)

## v1.2+ Requirements

Deferred to future milestone. Tracked but not in current roadmap.

### Alerting

- **ALERT-01**: Grafana alert rule fires when error rate exceeds threshold over 5 min
- **ALERT-02**: Grafana alert rule fires when p99 latency exceeds threshold over 5 min
- **ALERT-03**: Grafana alert rule fires when any backend is unhealthy for more than 2 min
- **ALERT-04**: Grafana alerts route to Discord via webhook contact point

### Decommission

- **DECOM-01**: ContextForge containers and images fully removed (after 2-week rollback window)
- **DECOM-02**: ContextForge Postgres data archived or deleted

## Out of Scope

| Feature | Reason |
|---------|--------|
| Blue/green deployment | Overkill for single-user localhost gateway, ~5s downtime acceptable |
| Alertmanager | Grafana built-in alerting + n8n health check provides dual-path without extra container |
| Loki log aggregation | `docker logs` + Postgres audit covers needs, Loki adds 500+ MB RAM |
| OpenTelemetry tracing | Single-hop architecture, no multi-service correlation needed |
| Automated rollback | Dangerous — incorrect health check = thrashing between services |
| Nginx/Traefik proxy | Caddy already handles reverse proxy, Sentinel is localhost-only |
| Public hostname/TLS | Claude Code runs on same VPS, no external access needed |
| cAdvisor resource monitoring | Existing container-health workflow covers container status |
| 20+ panel dashboard | 5 metric families = 5 panels, more is noise |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| PREP-01 | — | Pending |
| PREP-02 | — | Pending |
| PREP-03 | — | Pending |
| PREP-04 | — | Pending |
| PREP-05 | — | Pending |
| CUT-01 | — | Pending |
| CUT-02 | — | Pending |
| CUT-03 | — | Pending |
| CUT-04 | — | Pending |
| CUT-05 | — | Pending |
| NET-01 | — | Pending |
| NET-02 | — | Pending |
| NET-03 | — | Pending |
| NET-04 | — | Pending |
| MON-01 | — | Pending |
| MON-02 | — | Pending |
| MON-03 | — | Pending |
| MON-04 | — | Pending |
| MON-05 | — | Pending |
| MON-06 | — | Pending |
| OPS-01 | — | Pending |
| OPS-02 | — | Pending |
| OPS-03 | — | Pending |

**Coverage:**
- v1.1 requirements: 23 total
- Mapped to phases: 0
- Unmapped: 23 ⚠️

---
*Requirements defined: 2026-02-22*
*Last updated: 2026-02-22 after initial definition*
