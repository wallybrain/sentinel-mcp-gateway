# Requirements: Sentinel Gateway v1.1

**Defined:** 2026-02-22
**Core Value:** Every MCP tool call passes through one governed point with auth, audit, and rate limiting

## v1.1 Requirements

Requirements for Deploy & Harden milestone. Each maps to roadmap phases.

### Pre-Cutover Preparation

- [x] **PREP-01**: Port config is consistent across sentinel.toml, Dockerfile, and docker-compose.yml (resolve 9200/9201 drift)
- [x] **PREP-02**: Sidecar service definitions (mcp-n8n, mcp-sqlite) are migrated from ContextForge compose to Sentinel compose
- [x] **PREP-03**: Sentinel containers join a Docker network that can reach HTTP backends (mcp-n8n, mcp-sqlite) by hostname
- [x] **PREP-04**: Production .env file exists with JWT secret and Postgres credentials (not committed)
- [x] **PREP-05**: Sentinel containers start and pass health checks alongside running ContextForge

### Cutover

- [x] **CUT-01**: ContextForge gateway process is stopped (containers preserved for rollback, not `docker compose down`)
- [x] **CUT-02**: Sentinel handles MCP traffic via stdio transport (native binary) and serves health/metrics on port 9201
- [x] **CUT-03**: Claude Code MCP config updated with new Sentinel entry and fresh JWT token
- [x] **CUT-04**: All active backends respond to tool calls through Sentinel with durable env wiring (6 of 7; exa deferred to v1.2+ pending EXA_API_KEY)
- [x] **CUT-05**: Rollback procedure is documented and tested (ContextForge containers start and serve, Sentinel restored after reversal)

### Network Hardening

- [x] **NET-01**: Sentinel ports (9200, 9201) are bound to 127.0.0.1 only (verified unreachable from public IP)
- [x] **NET-02**: iptables DROP rules exist for ports 9200 and 9201 on eth0
- [x] **NET-03**: fix-iptables.sh is updated with Sentinel port rules
- [x] **NET-04**: Stale Docker networks from ContextForge are cleaned up

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
| Automated rollback | Dangerous -- incorrect health check = thrashing between services |
| Nginx/Traefik proxy | Caddy already handles reverse proxy, Sentinel is localhost-only |
| Public hostname/TLS | Claude Code runs on same VPS, no external access needed |
| cAdvisor resource monitoring | Existing container-health workflow covers container status |
| 20+ panel dashboard | 5 metric families = 5 panels, more is noise |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| PREP-01 | Phase 10 | Done |
| PREP-02 | Phase 10 | Done |
| PREP-03 | Phase 10 | Done |
| PREP-04 | Phase 10 | Done |
| PREP-05 | Phase 10 | Done |
| CUT-01 | Phase 15 (gap closure) | Done |
| CUT-02 | Phase 11 | Complete |
| CUT-03 | Phase 11 | Complete |
| CUT-04 | Phase 15 (gap closure) | Done |
| CUT-05 | Phase 15 (gap closure) | Done |
| NET-01 | Phase 12 | Complete |
| NET-02 | Phase 12 | Complete |
| NET-03 | Phase 12 | Complete |
| NET-04 | Phase 12 | Complete |
| MON-01 | Phase 13 | Pending |
| MON-02 | Phase 13 | Pending |
| MON-03 | Phase 13 | Pending |
| MON-04 | Phase 13 | Pending |
| MON-05 | Phase 13 | Pending |
| MON-06 | Phase 13 | Pending |
| OPS-01 | Phase 14 | Pending |
| OPS-02 | Phase 14 | Pending |
| OPS-03 | Phase 14 | Pending |

**Coverage:**
- v1.1 requirements: 23 total
- Mapped to phases: 23
- Unmapped: 0

---
*Requirements defined: 2026-02-22*
*Last updated: 2026-02-22 after milestone audit gap closure planning*
