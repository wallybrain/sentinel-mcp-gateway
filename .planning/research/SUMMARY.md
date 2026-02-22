# Project Research Summary

**Project:** Sentinel Gateway v1.1 -- Deploy, Monitor & Harden
**Domain:** MCP gateway production deployment and cutover on shared VPS
**Researched:** 2026-02-22
**Confidence:** HIGH

## Executive Summary

Sentinel Gateway v1.1 is a deployment and cutover project, not a feature build. The v1.0 Rust binary is complete (138 tests, 47 requirements, 3,776 LOC). The work is replacing the running ContextForge Python gateway with Sentinel on a shared VPS that hosts 14 containers, then layering Prometheus/Grafana monitoring and n8n health checks on top. The core challenge is surgical: swap one live gateway for another without breaking Claude Code's MCP toolchain or the VPS's existing services.

The recommended approach is a strict sequential cutover: fix port config drift, migrate sidecar ownership, deploy Sentinel alongside ContextForge on a temporary port, verify all 7 backends, stop ContextForge, rebind Sentinel to port 9200, update Claude Code's MCP config with a new JWT, then add monitoring. This order is driven by hard dependencies -- monitoring is useless until the gateway is live, and the gateway cannot go live until Docker networking is resolved. The net result is fewer containers (14 to 13), less RAM (~190 MB savings from removing ContextForge's Redis and Python stack), and full observability via 5 built-in Prometheus metrics that already exist in the binary.

The primary risks are all in the cutover sequence. ContextForge owns the sidecar containers (mcp-n8n, mcp-sqlite) that Sentinel needs -- running `docker compose down` kills them. The port config has drift between sentinel.toml (9200), Dockerfile (9201), and docker-compose.yml (9201). The JWT claims differ between gateways (different issuer/audience). Claude Code's MCP entry launches a ContextForge-specific Python wrapper that cannot simply be re-pointed at Sentinel. All four problems are preventable with preparation but catastrophic if missed. The rollback path is straightforward: restart ContextForge, revert MCP config.

## Key Findings

### Recommended Stack

No new languages or frameworks. The additions are two monitoring containers (Prometheus 3.5.1 LTS + Grafana 12.3) and one n8n workflow. Total new RAM: ~150 MB. The existing `/metrics` endpoint on Sentinel already exposes 5 metric families in Prometheus text format -- zero code changes needed.

**Core technologies:**
- **Prometheus 3.5.1 LTS**: metrics scraping -- scrapes Sentinel's existing `/metrics` endpoint, 10-15s interval, 30-day retention capped at 1 GB (~50-100 MB actual)
- **Grafana 12.3**: dashboards and alerting -- provisioned via YAML, Discord webhook contact point for alerts, behind Caddy/Authelia for browser access at 127.0.0.1:3100
- **n8n (existing)**: health monitoring -- new workflow polls `/health` and `/ready` every 2 min, Discord alert on failure, matches established VPS monitoring pattern
- **Docker Compose v2 (existing)**: orchestration -- extend current compose with sentinelnet network, add Prometheus/Grafana services

**Explicitly not adding:** Loki (docker logs + Postgres audit sufficient), Tempo (single-hop, no distributed tracing needed), cAdvisor (existing container-health workflow covers status), AlertManager (Grafana built-in alerting handles Discord), postgres_exporter (audit-only DB, not worth a dedicated exporter), OpenTelemetry (deferred to v2), Nginx/Traefik (Caddy already handles reverse proxy).

### Expected Features

**Must have (table stakes):**
- Clean ContextForge-to-Sentinel cutover with documented rollback plan
- Claude MCP config update with new JWT (issuer=`sentinel-gateway`, audience=`sentinel-api`)
- End-to-end verification of all 7 backends through Sentinel
- 127.0.0.1 binding verification + iptables hardening on eth0
- n8n health check workflow with Discord alerts (poll every 2 min)
- Prometheus scraping + Grafana dashboard (5 panels: request rate, error rate, latency percentiles, backend health, rate limit hits)

**Should have (after 24h stable):**
- Grafana alert rules for metric thresholds (error rate > 10%, p99 > 30s, backend unhealthy > 2 min)
- Audit log rotation (DELETE older than 30 days)
- Nightly Postgres backup (extend existing backup script with pg_dump)
- VPS reboot restart verification

**Defer (v2+):**
- OpenTelemetry tracing -- only if multi-hop routing added
- AlertManager -- only if alert routing exceeds Grafana rules
- Loki log aggregation -- only if `docker logs` proves insufficient
- cAdvisor container metrics -- only if resource usage becomes a concern

### Architecture Approach

The target architecture replaces ContextForge's 5-container stack (gateway + postgres + redis + mcp-n8n + mcp-sqlite) with Sentinel's stack (gateway + postgres), reusing the existing mcp-n8n and mcp-sqlite sidecars by migrating their service definitions. A new `sentinelnet` Docker network replaces `mcpnet`. The gateway also joins `n8nnet` so n8n health workflows can reach `/health`. Prometheus and Grafana are added to `sentinelnet`. All host-bound ports use 127.0.0.1 exclusively.

**Major components:**
1. **sentinel-gateway** -- Rust binary on 127.0.0.1:9200 (MCP) + 127.0.0.1:9201 (health/metrics), handles JWT auth, RBAC, rate limiting, routing to 2 HTTP + 5 stdio backends
2. **sentinel-postgres** -- Postgres 16-alpine for audit logs, Docker-internal port only (not exposed to host)
3. **prometheus** -- scrapes `/metrics` every 15s, 30-day/1GB retention cap, Docker-internal only (127.0.0.1:9090)
4. **grafana** -- 5-panel dashboard + Discord alerting, 127.0.0.1:3100, proxied through Caddy with Authelia 2FA
5. **n8n health workflow** -- polls `/health` and `/ready` every 2 min, Discord alerts on failure/recovery, independent from Prometheus path

### Critical Pitfalls

1. **Sidecar ownership (Pitfall 4)** -- `docker compose down` in ContextForge dir kills mcp-n8n and mcp-sqlite, which Sentinel needs. Migrate sidecar definitions into Sentinel's compose BEFORE stopping ContextForge. Use `docker compose stop gateway` (not `down`) during transition.

2. **Port config drift (Pitfall 1)** -- sentinel.toml says 9200, Dockerfile EXPOSEs 9201, compose maps 9201. All three must agree BEFORE deployment. Verify with `curl /health` after every container restart.

3. **Docker network isolation (Pitfall 3)** -- Sentinel cannot reach mcp-n8n or mcp-sqlite unless they share a Docker network. Define `sentinelnet`, ensure both sidecars join it, verify DNS with `docker exec sentinel-gateway nslookup mcp-n8n`.

4. **MCP config mismatch (Pitfall 2)** -- Claude Code's MCP entry launches a ContextForge-specific Python wrapper with wrong JWT claims (issuer=`mcpgateway`). Need a completely new MCP config entry with Sentinel-native integration, not just a port change.

5. **iptables bridge ID drift (Pitfall 6)** -- Docker bridge interface names change on network recreation. Re-run `fix-iptables.sh` after any `docker compose down`/`up` cycle that recreates networks.

## Implications for Roadmap

Based on research, suggested 5-phase structure. Phases 1-3 are the critical path (strictly sequential). Phase 4 follows. Phase 5 is gated on 24h stability.

### Phase 1: Pre-Cutover Preparation
**Rationale:** All critical pitfalls require preparation before any containers start. Port config, sidecar ownership, network topology, and Postgres strategy must be resolved first. This is the highest-risk phase because mistakes here cascade into every subsequent phase.
**Delivers:** Fixed sentinel.toml/Dockerfile/compose port agreement, sidecar service definitions (mcp-n8n, mcp-sqlite) migrated to Sentinel's compose, sentinelnet + n8nnet network config, production .env file with JWT secret and Postgres password, Sentinel containers running on temporary port alongside ContextForge, health endpoint verified.
**Addresses:** Port config drift fix, Docker networking setup, sidecar ownership transfer, Postgres strategy (separate instances)
**Avoids:** Pitfalls 1 (port collision), 3 (network isolation), 4 (sidecar killed), 5 (Postgres conflict)

### Phase 2: Cutover Execution
**Rationale:** Cannot monitor what is not running. Cutover is the critical path -- everything else depends on Sentinel being the live gateway. Must verify all backends before committing, and must have rollback tested before decommitting ContextForge.
**Delivers:** Sentinel running on port 9200, ContextForge gateway stopped (images/volumes preserved for rollback), Claude Code MCP config updated with new JWT and Sentinel-native entry, all 7 backends verified end-to-end with real tool calls, rollback plan documented and tested.
**Addresses:** Clean cutover, MCP config update, JWT generation, end-to-end verification, rollback documentation
**Avoids:** Pitfalls 2 (MCP config mismatch), 7 (stdio backends in Docker)

### Phase 3: Network Hardening
**Rationale:** Security verification immediately after cutover, before adding more containers that change the network topology. Short phase but critical for defense-in-depth.
**Delivers:** Verified 127.0.0.1 binding on all Sentinel ports (9200, 9201), iptables DROP rules for 9200/9201 on eth0, updated fix-iptables.sh with Sentinel ports, old mcpnet network cleaned up, external port scan confirming nothing leaked.
**Addresses:** Binding verification, iptables hardening, stale network cleanup
**Avoids:** Pitfall 6 (iptables bridge ID drift)

### Phase 4: Monitoring Stack
**Rationale:** Best after Phase 3 (gateway is live with real traffic, security is verified). Two independent monitoring tracks: Prometheus/Grafana for metrics visualization and alerting, n8n for binary health polling. Can be built in parallel within the phase.
**Delivers:** Prometheus scraping `/metrics` every 15s, Grafana with 5-panel dashboard (request rate, error rate, latency histogram, backend health, rate limit hits), provisioned datasource, n8n "Sentinel Gateway Health" workflow polling `/health` and `/ready` every 2 min with Discord alerts.
**Uses:** Prometheus 3.5.1, Grafana 12.3, existing n8n + Discord webhook
**Addresses:** Prometheus config, Grafana dashboard and provisioning, n8n health check, dual-path alerting (metrics + health)

### Phase 5: Hardening and Polish
**Rationale:** Only after the gateway is live, monitored, and stable for 24+ hours. These items are operational polish that prevent alert fatigue if added too early during initial tuning.
**Delivers:** Grafana alert rules (error rate > 10%, p99 > 30s, backend unhealthy > 2 min) with Discord contact point, audit log rotation (30-day retention via cron or n8n), nightly Postgres backup (extend existing backup script), VPS reboot restart verification, ContextForge full decommission (remove containers, images, optionally Postgres volume).
**Addresses:** Grafana alerting, audit log rotation, backup integration, reboot test, final ContextForge cleanup

### Phase Ordering Rationale

- **Phases 1-2-3 are strictly sequential.** You cannot cut over without preparation (sidecar migration, port fix, network setup). You cannot harden until the cutover is complete and the network topology is final.
- **Phase 4 follows Phase 3** because monitoring containers add to the network topology. Hardening first means you verify security before adding Prometheus/Grafana, not after.
- **Phase 5 is gated on 24h stability.** Adding Grafana alert rules during the first hours of operation guarantees false positives from tuning noise. Wait for baseline data.
- **Sidecar migration (Phase 1) is the single highest-risk item.** If mcp-n8n and mcp-sqlite are not properly transferred to Sentinel's compose, the entire cutover fails. This must be resolved first and verified independently.
- **n8n health check is the fastest path to alerting** -- it does not need Prometheus/Grafana. But it needs the gateway on a stable port, so it goes in Phase 4 (after cutover), not Phase 1.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Cutover Execution):** The Claude Code MCP config entry needs investigation. The current entry launches a ContextForge-specific Docker wrapper with a Python image. Sentinel needs its own integration -- either direct stdio, a Rust wrapper, or the Docker wrapper pattern with a different image. Verify how Sentinel exposes itself to Claude Code's stdio transport.
- **Phase 4 (Monitoring Stack):** The Grafana dashboard JSON must be authored from scratch. Consider building in Grafana UI first, then exporting as provisioned JSON. The n8n workflow needs specific node configuration (HTTP Request + IF + Discord).

Phases with standard patterns (skip research-phase):
- **Phase 1 (Pre-Cutover Preparation):** Docker Compose networking, port config fixes, and service definition migration are all mechanical and well-documented.
- **Phase 3 (Network Hardening):** Follows the exact same iptables + binding verification pattern used for every other VPS service. Copy from fix-iptables.sh.
- **Phase 5 (Hardening and Polish):** Grafana alerting, pg_dump backups, cron jobs, and log rotation are all established patterns on this VPS.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All technologies already running on VPS (n8n, Docker) or have well-documented Docker images (Prometheus, Grafana). No new languages or frameworks. |
| Features | HIGH | Feature list derived from actual infrastructure docs, existing VPS monitoring patterns, and running ContextForge deployment. No speculative features. |
| Architecture | HIGH | Based on reading actual docker-compose files, Dockerfiles, sentinel.toml, and Docker network topology from running containers. Network diagram verified against `docker network ls`. |
| Pitfalls | HIGH | Every pitfall grounded in specific config files (sentinel.toml line showing 9200, Dockerfile EXPOSE showing 9201, compose port mapping showing 9201). No hypothetical risks -- all are config drift or ownership issues visible in the codebase. |

**Overall confidence:** HIGH

### Gaps to Address

- **Claude Code MCP integration mechanism:** The exact format of the new MCP config entry is unresolved. ContextForge uses a Python wrapper in a Docker container that speaks its own HTTP protocol. Sentinel may need direct stdio, a Rust wrapper binary, or the same Docker pattern with a different image. Needs validation during Phase 2 planning.

- **mcp-n8n and mcp-sqlite service definitions:** These are currently defined in ContextForge's docker-compose.yml. The exact service definitions (image, volumes, environment, networks) need to be extracted and replicated in Sentinel's compose. Review `/home/lwb3/mcp-context-forge/docker-compose.slim.yml` during Phase 1 planning.

- **Grafana dashboard JSON:** No pre-built dashboard exists. Must be authored from scratch or built in Grafana UI and exported. Low risk but non-trivial time (~1-2 hours for 5 panels with proper PromQL).

- **Playwright in Docker:** The Sentinel Dockerfile may or may not include Chromium for the Playwright MCP server (~400 MB). If missing, Playwright tools fail silently (gateway healthy, tools listed, calls error). Needs verification during Phase 2.

- **n8nnet network name:** The n8n compose project creates a network -- the actual name may be `n8n-mcp_default` rather than `n8nnet`. Verify with `docker network ls` before writing the sentinelnet config.

## Sources

### Primary (HIGH confidence)
- `/home/lwb3/sentinel-gateway/docker-compose.yml` -- current compose config, port 9201 mapping, healthchecks
- `/home/lwb3/sentinel-gateway/sentinel.toml` -- gateway config, listen 127.0.0.1:9200, backend definitions
- `/home/lwb3/sentinel-gateway/Dockerfile` -- EXPOSE 9201, healthcheck on 9201, multi-stage Rust build
- `/home/lwb3/sentinel-gateway/docs/CURRENT-INFRASTRUCTURE.md` -- full VPS container map (14 containers)
- `/home/lwb3/sentinel-gateway/docs/MCP-TOPOLOGY.md` -- Docker network topology (mcpnet, n8nnet, webproxy)
- `/home/lwb3/sentinel-gateway/docs/CONTEXTFORGE-GATEWAY.md` -- ContextForge deployment, JWT config, tool catalog
- `/home/lwb3/mcp-context-forge/docker-compose.slim.yml` -- ContextForge slim overrides, port 9200, Postgres 5434
- `/home/lwb3/v1be-code-server/fix-iptables.sh` -- iptables bridge ID drift documentation
- Sentinel Gateway source: `src/metrics/mod.rs`, `src/health/server.rs` -- 5 metric families, health endpoint

### Secondary (MEDIUM confidence)
- [Prometheus 3.5.1 LTS release](https://github.com/prometheus/prometheus/releases) -- current stable
- [Grafana 12.3 documentation](https://grafana.com/docs/grafana/latest/) -- provisioning, alerting, Docker deployment
- [Grafana provisioning docs](https://grafana.com/docs/grafana/latest/administration/provisioning/) -- datasource + dashboard YAML
- [Prometheus storage sizing](https://prometheus.io/docs/prometheus/latest/storage/) -- ~1-2 bytes per sample
- [n8n health monitoring workflow template](https://n8n.io/workflows/8412-website-and-api-health-monitoring-system-with-http-status-validation/)

### Tertiary (LOW confidence)
- RAM estimates for Prometheus (~80 MB) and Grafana (~60 MB) are approximations from community benchmarks, not measured on this VPS

---
*Research completed: 2026-02-22*
*Ready for roadmap: yes*
