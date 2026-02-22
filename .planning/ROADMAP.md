# Roadmap: Sentinel Gateway

**Created:** 2026-02-22

## Milestones

- ✅ **v1.0 Sentinel Gateway MVP** -- Phases 1-9 (shipped 2026-02-22)
- **v1.1 Deploy & Harden** -- Phases 10-15 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-9) -- SHIPPED 2026-02-22</summary>

- [x] Phase 1: Foundation & Config (2/2 plans) -- completed 2026-02-22
- [x] Phase 2: MCP Protocol Layer (2/2 plans) -- completed 2026-02-22
- [x] Phase 3: HTTP Backend Routing (2/2 plans) -- completed 2026-02-22
- [x] Phase 4: Authentication & Authorization (2/2 plans) -- completed 2026-02-22
- [x] Phase 5: Audit Logging (2/2 plans) -- completed 2026-02-22
- [x] Phase 6: Rate Limiting & Kill Switch (2/2 plans) -- completed 2026-02-22
- [x] Phase 7: Health & Reliability (2/2 plans) -- completed 2026-02-22
- [x] Phase 8: stdio Backend Management (3/3 plans) -- completed 2026-02-22
- [x] Phase 9: Observability & Hot Reload (3/3 plans) -- completed 2026-02-22

**47/47 requirements satisfied. 138 tests. 3,776 LOC Rust.**
**Full archive:** `milestones/v1.0-ROADMAP.md`

</details>

### v1.1 Deploy & Harden (In Progress)

**Milestone Goal:** Replace ContextForge with Sentinel on the VPS, add monitoring and network hardening.

- [x] **Phase 10: Pre-Cutover Preparation** - Fix port drift, migrate sidecars, deploy Sentinel alongside ContextForge
- [x] **Phase 11: Cutover Execution** - Stop ContextForge, register native binary, verify all backends (**completed 2026-02-22**)
- [x] **Phase 12: Network Hardening** - Lock down 127.0.0.1 binding, iptables rules, clean stale networks (completed 2026-02-22)
- [ ] **Phase 13: Monitoring Stack** - Prometheus + Grafana dashboard + n8n health checks with Discord alerts
- [ ] **Phase 14: Operations** - Audit log rotation, backup integration, reboot resilience
- [x] **Phase 15: Cutover Gap Closure** - Fix audit gaps: rollback test, Firecrawl key wiring, config hardening, doc sync (**completed 2026-02-22**)

## Phase Details

### Phase 10: Pre-Cutover Preparation
**Goal**: Sentinel containers run healthy alongside ContextForge with all config drift resolved and sidecars migrated
**Depends on**: v1.0 shipped
**Requirements**: PREP-01, PREP-02, PREP-03, PREP-04, PREP-05
**Success Criteria** (what must be TRUE):
  1. Port references agree across sentinel.toml, Dockerfile, and docker-compose.yml (no 9200/9201 drift)
  2. mcp-n8n and mcp-sqlite service definitions exist in Sentinel's docker-compose.yml and containers start from that compose
  3. Sentinel gateway can reach mcp-n8n and mcp-sqlite by hostname over a shared Docker network
  4. Production .env with JWT secret and Postgres credentials exists on VPS (not in git)
  5. Sentinel health endpoint returns 200 while ContextForge is still running on port 9200
**Plans**: 2 plans

Plans:
- [x] 10-01-PLAN.md -- Port fix, sidecar migration, Docker networking, production .env
- [x] 10-02-PLAN.md -- Build and start Sentinel stack alongside ContextForge, verify health

### Phase 11: Cutover Execution
**Goal**: Sentinel is the live MCP gateway (native binary, stdio transport) with all backends verified and rollback tested
**Depends on**: Phase 10
**Requirements**: CUT-01, CUT-02, CUT-03, CUT-04, CUT-05
**Success Criteria** (what must be TRUE):
  1. ContextForge gateway container is stopped (not removed -- images and volumes preserved for rollback)
  2. Sentinel handles MCP traffic via stdio (native binary spawned by Claude Code) and serves health/metrics on port 9201
  3. Claude Code MCP config references the native Sentinel binary with a valid JWT token
  4. Tool calls to governed backends (n8n, sqlite) succeed end-to-end through Sentinel
  5. Rollback procedure is documented and has been tested (restart ContextForge, revert MCP config)
**Plans**: 2 plans

Plans:
- [x] 11-01-PLAN.md -- Build binary, update compose + config, expose ports, generate token
- [x] 11-02-PLAN.md -- Stop ContextForge, register MCP server, verify backends, document rollback

### Phase 12: Network Hardening
**Goal**: Sentinel ports are unreachable from the public internet, verified by external scan
**Depends on**: Phase 11
**Requirements**: NET-01, NET-02, NET-03, NET-04
**Success Criteria** (what must be TRUE):
  1. Ports 9200 and 9201 are bound to 127.0.0.1 only (curl from public IP times out or refuses)
  2. iptables DROP rules block ports 9200 and 9201 on eth0
  3. fix-iptables.sh includes Sentinel port rules and produces correct state when re-run
  4. Stale ContextForge Docker networks (mcpnet) are removed
**Plans**: 1 plan

Plans:
- [x] 12-01-PLAN.md -- Verify port binding, add iptables DROP rules, update fix-iptables.sh, remove stale Docker network

### Phase 13: Monitoring Stack
**Goal**: Sentinel metrics are visualized in Grafana and health failures trigger Discord alerts within minutes
**Depends on**: Phase 12
**Requirements**: MON-01, MON-02, MON-03, MON-04, MON-05, MON-06
**Success Criteria** (what must be TRUE):
  1. Prometheus scrapes Sentinel /metrics endpoint on a regular interval and stores time series data
  2. Grafana loads with a pre-provisioned Prometheus datasource (no manual configuration needed)
  3. Grafana dashboard displays 5 panels: request rate, error rate, latency percentiles, backend health, rate limit hits
  4. When Sentinel health check fails, a Discord alert fires within 5 minutes
  5. When Sentinel recovers from a failure, a Discord recovery notification fires
**Plans**: TBD

Plans:
- [ ] 13-01: TBD
- [ ] 13-02: TBD

### Phase 14: Operations
**Goal**: Sentinel runs unattended with automatic log rotation, backups, and reboot recovery
**Depends on**: Phase 13 (gated on 24h stability after monitoring is live)
**Requirements**: OPS-01, OPS-02, OPS-03
**Success Criteria** (what must be TRUE):
  1. Audit log records older than 30 days are automatically deleted on a schedule
  2. Nightly backup script includes a pg_dump of Sentinel's Postgres database
  3. After a VPS reboot, all Sentinel containers (gateway, postgres, sidecars, monitoring) restart automatically without manual intervention
**Plans**: TBD

Plans:
- [ ] 14-01: TBD

### Phase 15: Cutover Gap Closure
**Goal**: All cutover audit gaps closed — rollback tested, env wiring durable, config explicit, docs accurate
**Depends on**: Phase 12 (can run before or after 13/14)
**Requirements**: CUT-01, CUT-04, CUT-05
**Gap Closure**: Closes gaps from v1.1-MILESTONE-AUDIT.md
**Success Criteria** (what must be TRUE):
  1. Rollback procedure has been tested end-to-end (start ContextForge, verify MCP traffic, reverse)
  2. FIRECRAWL_API_KEY is in the MCP env block in add-mcp.sh (durable wiring, not inherited env)
  3. health_listen is explicit in sentinel.toml (not relying on hardcoded default)
  4. Duplicate MCP registration removed (single scope in ~/.claude.json)
  5. Orphaned sentinel-docker.toml removed
  6. REQUIREMENTS.md checkboxes and text updated for CUT-01, CUT-04, CUT-05
**Plans**: 2 plans

Plans:
- [x] 15-01-PLAN.md -- Config hardening, env wiring, dead file cleanup, doc sync
- [x] 15-02-PLAN.md -- Rollback test execution and CUT-05 verification

## Progress

**Execution Order:**
Phases execute in numeric order: 10 -> 11 -> 12 -> 15 -> 13 -> 14

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Foundation & Config | v1.0 | 2/2 | Complete | 2026-02-22 |
| 2. MCP Protocol Layer | v1.0 | 2/2 | Complete | 2026-02-22 |
| 3. HTTP Backend Routing | v1.0 | 2/2 | Complete | 2026-02-22 |
| 4. Authentication & Authorization | v1.0 | 2/2 | Complete | 2026-02-22 |
| 5. Audit Logging | v1.0 | 2/2 | Complete | 2026-02-22 |
| 6. Rate Limiting & Kill Switch | v1.0 | 2/2 | Complete | 2026-02-22 |
| 7. Health & Reliability | v1.0 | 2/2 | Complete | 2026-02-22 |
| 8. stdio Backend Management | v1.0 | 3/3 | Complete | 2026-02-22 |
| 9. Observability & Hot Reload | v1.0 | 3/3 | Complete | 2026-02-22 |
| 10. Pre-Cutover Preparation | v1.1 | 2/2 | Complete | 2026-02-22 |
| 11. Cutover Execution | v1.1 | 2/2 | Complete | 2026-02-22 |
| 12. Network Hardening | v1.1 | 1/1 | Complete | 2026-02-22 |
| 13. Monitoring Stack | v1.1 | 0/TBD | Not started | - |
| 14. Operations | v1.1 | 0/TBD | Not started | - |
| 15. Cutover Gap Closure | v1.1 | 2/2 | Complete | 2026-02-22 |

---
*Roadmap created: 2026-02-22*
*Last updated: 2026-02-22 after Phase 15 planning*
