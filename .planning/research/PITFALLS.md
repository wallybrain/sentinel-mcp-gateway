# Pitfalls Research

**Domain:** Replacing ContextForge with Sentinel Gateway on a shared VPS (deployment, cutover, monitoring)
**Researched:** 2026-02-22
**Confidence:** HIGH (grounded in actual infrastructure docs, docker-compose files, and known VPS state)

## Critical Pitfalls

### Pitfall 1: Port 9200 Collision During Cutover

**What goes wrong:**
ContextForge binds `127.0.0.1:9200 -> 4444`. Sentinel's docker-compose maps `127.0.0.1:9201:9201`. But `sentinel.toml` says `listen = "127.0.0.1:9200"` -- a mismatch with the Dockerfile which EXPOSEs 9201 and healthchecks on 9201. If someone "fixes" this by making Sentinel also listen on 9200, both stacks try to bind the same host port. Docker fails silently on the second bind -- the container starts but no traffic reaches it. Claude Code MCP config still points at 9200, so it keeps hitting ContextForge (or nothing).

**Why it happens:**
The port was changed during development (9200 in config, 9201 in Dockerfile/compose) creating drift. During cutover, the instinct is "just change it to 9200" without stopping ContextForge first. Docker port bind errors are easy to miss in `docker compose up -d` output.

**How to avoid:**
1. Fix the config mismatch FIRST: either `sentinel.toml` listens on 9201 (matching Dockerfile) or all three agree on 9200
2. Follow a strict cutover sequence: stop ContextForge -> verify port free -> start Sentinel -> verify health -> update MCP config
3. After `docker compose up -d`, always run `curl http://127.0.0.1:9200/health` (or 9201) to verify the correct service answers
4. Check `docker compose logs sentinel-gateway` for bind errors -- they show up as "address already in use"

**Warning signs:**
- `sentinel.toml` listen address differs from Dockerfile EXPOSE and healthcheck
- `curl /health` returns ContextForge's response format (Python JSON) instead of Sentinel's
- Container shows "Up" but healthcheck fails

**Phase to address:**
Pre-cutover preparation (Phase 1 of v1.1 roadmap)

---

### Pitfall 2: Claude Code MCP Config Points at Dead Wrapper

**What goes wrong:**
Claude Code's `~/.claude/settings.json` references `sentinel-gateway` as an MCP server, but it launches a Docker wrapper container (`python3 -m mcpgateway.wrapper`) from the ContextForge image. This wrapper speaks ContextForge's specific HTTP protocol to port 9200. Sentinel has a different auth flow (different JWT issuer/audience: `sentinel-gateway`/`sentinel-api` vs `mcpgateway`/`mcpgateway-api`), different endpoint paths, and different tool names. Simply changing the port is not enough -- the wrapper itself is ContextForge-specific.

**Why it happens:**
The MCP config entry looks opaque (a Docker command with env vars). It is easy to think "just change the port" without realizing the wrapper, JWT claims, and endpoint structure all need to change.

**How to avoid:**
1. Sentinel needs its own Claude Code integration -- either a native stdio binary, a thin wrapper, or direct HTTP config
2. Generate a new JWT with issuer=`sentinel-gateway`, audience=`sentinel-api` BEFORE cutover
3. Test the new MCP config entry in isolation: start Sentinel, configure Claude Code to use it (under a different server name), verify `tools/list` returns all 19+ tools
4. Only THEN remove the old `sentinel-gateway` config entry (which actually points at ContextForge)
5. Keep ContextForge config as `mcp__contextforge__*` as a rollback option during testing

**Warning signs:**
- `tools/list` returns 0 tools or auth errors after config change
- JWT rejection errors in Sentinel logs (wrong issuer/audience)
- Claude Code shows "MCP server sentinel-gateway failed to start"

**Phase to address:**
Cutover execution (Phase 2 of v1.1 roadmap)

---

### Pitfall 3: Docker Network Isolation Breaks HTTP Backend Routing

**What goes wrong:**
Sentinel's docker-compose does not define or join the `mcpnet` network. The current ContextForge stack has `mcp-n8n` and `mcp-sqlite` containers on `mcpnet`. If Sentinel starts in its own default network, it cannot reach `http://mcp-n8n:3000` or `http://mcp-sqlite:3000` because Docker DNS resolution only works within shared networks. The `sentinel.toml` config hardcodes these hostnames.

**Why it happens:**
Sentinel was developed locally with its own docker-compose.yml that only defines `gateway` and `postgres`. The HTTP backend containers (`mcp-n8n`, `mcp-sqlite`) belong to ContextForge's compose project. Docker Compose creates isolated networks per project by default.

**How to avoid:**
1. Sentinel's docker-compose must explicitly join the `mcpnet` network (or a new shared network that both projects use)
2. Alternatively, use an external network declaration: define `mcpnet` as external in Sentinel's compose, so both projects share it
3. Verify DNS resolution from inside the Sentinel container: `docker exec sentinel-gateway nslookup mcp-n8n`
4. `mcp-n8n` also bridges to `n8nnet` (to reach n8n at port 5678) -- this bridge must survive the ContextForge teardown. If `mcp-n8n` is defined in ContextForge's compose, stopping ContextForge kills it. Sentinel needs to own or co-own these sidecar containers.

**Warning signs:**
- Sentinel health is green but all HTTP tool calls fail with connection refused
- `"error": "Connection refused"` in Sentinel logs for mcp-n8n or mcp-sqlite
- `docker network inspect mcpnet` shows Sentinel is not a member

**Phase to address:**
Pre-cutover preparation (Phase 1 of v1.1 roadmap) -- network topology must be planned before any containers start

---

### Pitfall 4: Stopping ContextForge Kills Shared Sidecar Containers

**What goes wrong:**
Running `docker compose down` in `/home/lwb3/mcp-context-forge/` stops ALL services defined in ContextForge's docker-compose: gateway, postgres, redis, AND `mcp-n8n` AND `mcp-sqlite`. These sidecar containers are the HTTP backends that Sentinel needs. If ContextForge "owns" them in its compose file, tearing down ContextForge tears down Sentinel's backends.

**Why it happens:**
Docker Compose manages the full lifecycle of all services in a compose file. There is no "stop this one service but keep the others" at the project level (you can stop individual services, but `down` kills everything). The sidecars were originally created as part of ContextForge's stack.

**How to avoid:**
1. BEFORE stopping ContextForge, migrate sidecar ownership: move `mcp-n8n` and `mcp-sqlite` service definitions into Sentinel's docker-compose.yml (or a shared compose file)
2. Use `docker compose stop gateway` instead of `docker compose down` -- this stops only the gateway container, leaving sidecars running
3. Better: extract sidecars into their own compose file (`docker-compose.backends.yml`) that both Sentinel and ContextForge can reference
4. Test the full sequence in order: start Sentinel stack (with sidecars) -> verify backends reachable -> stop ContextForge gateway only -> verify Sentinel still works -> then clean up ContextForge

**Warning signs:**
- After `docker compose down` in ContextForge dir, `docker ps` shows mcp-n8n and mcp-sqlite are gone
- Sentinel logs show connection refused to backends that were working moments ago

**Phase to address:**
Pre-cutover preparation (Phase 1 of v1.1 roadmap)

---

### Pitfall 5: Postgres Data Conflict -- Separate vs Shared

**What goes wrong:**
ContextForge uses Postgres 18 (`mcp-context-forge-postgres-1`) with database `mcp`, user `postgres`, on port `127.0.0.1:5434`. Sentinel uses Postgres 16-alpine (`sentinel-postgres`) with database `sentinel`, user `sentinel`, on an internal port. Running both creates TWO Postgres instances consuming ~200 MB RAM total on a 16 GB VPS with 14 other containers. Worse, if someone decides to "share" the Postgres instance, Sentinel's `sqlx` embedded migrations assume a clean database and may conflict with ContextForge's Alembic-managed schema.

**Why it happens:**
Sharing a database sounds efficient. But the schemas are incompatible (different ORMs, different migration tools, different table names). The temptation to share grows when RAM is constrained.

**How to avoid:**
1. Use SEPARATE Postgres instances during cutover -- Sentinel gets its own, ContextForge keeps its own
2. After ContextForge is fully decommissioned (weeks later, after confidence builds), remove ContextForge's Postgres
3. Never share a Postgres instance between projects with different migration tools (Alembic vs sqlx)
4. If RAM is a concern, reduce ContextForge Postgres `shared_buffers` to 64 MB during the transition period
5. Sentinel's Postgres can use Postgres 16-alpine (smaller image, less RAM) -- no need to match ContextForge's Postgres 18

**Warning signs:**
- Two `postgres` containers running simultaneously
- OOM kills on the VPS during peak usage
- Migration errors mentioning tables that belong to the other project

**Phase to address:**
Pre-cutover preparation (Phase 1) -- decide and document the Postgres strategy before deployment

---

### Pitfall 6: iptables Rules Reference Stale Docker Bridge IDs

**What goes wrong:**
The VPS has iptables rules that reference specific Docker bridge network IDs (e.g., `br-03b170a2a124` for webproxy). These bridge interface names are generated by Docker and change when networks are recreated. Stopping and recreating ContextForge's `mcpnet` (which happens on `docker compose down` + `up`) generates a new bridge ID. Any iptables rules referencing the old bridge ID silently stop matching, opening or closing ports unexpectedly.

**Why it happens:**
Docker bridge names are derived from the network's internal ID, which is assigned at creation time. `docker compose down` destroys the network; `docker compose up` creates a new one with a new ID. The existing `fix-iptables.sh` at `/home/lwb3/v1be-code-server/fix-iptables.sh` already documents this problem for the webproxy network.

**How to avoid:**
1. After any `docker compose down`/`up` cycle, re-run `fix-iptables.sh` (or update it for the new bridge names)
2. Use `docker network ls` to find the new bridge interface name, then update iptables rules
3. Better: use Docker's own network isolation (internal networks, no port publishing) instead of iptables rules for container-to-container traffic
4. For Sentinel specifically: since it binds to `127.0.0.1` only, it does not need new iptables rules -- but verify that existing DROP rules on eth0 are still intact after the network churn
5. Consider using `--internal` flag on Docker networks that should never reach the internet (mcpnet does not need egress)

**Warning signs:**
- `iptables -L -v` shows rules with interface names that do not match `ip link show` output
- Services that were blocked become accessible from the internet
- `fix-iptables.sh` has hardcoded bridge IDs that no longer exist

**Phase to address:**
Post-cutover verification (Phase 3 of v1.1 roadmap)

---

### Pitfall 7: stdio Backends Inside Docker Cannot Access Host Resources

**What goes wrong:**
Sentinel's Dockerfile installs Node.js MCP servers and spawns them as stdio children inside the container. But some backends need host resources: `playwright` needs a browser (Chromium), `firecrawl` needs internet access (may be restricted by Docker network policy), `exa` needs an API key from the host environment. If the container's network mode is not `host` and packages are not properly installed, stdio backends fail at runtime despite the gateway being healthy.

**Why it happens:**
The Dockerfile copies the Rust binary and config but the npm packages for stdio backends need to be installed during the Docker build. Browser binaries for Playwright are large (~400 MB) and may not be included. API keys need to be passed as environment variables through docker-compose.

**How to avoid:**
1. Install ALL npm-based MCP servers in the Dockerfile: `npm install -g @upstash/context7-mcp firecrawl-mcp exa-mcp-server @modelcontextprotocol/server-sequential-thinking @anthropic/mcp-playwright`
2. For Playwright: either install Chromium in the Docker image (large, ~400 MB) or run Playwright as a separate container with its own browser
3. Pass ALL required API keys (Firecrawl, Exa) as environment variables in docker-compose.yml
4. Test EACH stdio backend individually after deployment: call one tool from each backend and verify success
5. Consider running the gateway with `--network=host` to avoid Docker networking complexity for stdio backends that need internet access

**Warning signs:**
- Gateway starts but `tools/list` returns only HTTP backend tools (n8n, sqlite), missing stdio backend tools
- Stdio backend logs show "module not found" or "ENOENT" errors
- Playwright tools fail with "browser not found"

**Phase to address:**
Deployment (Phase 2 of v1.1 roadmap)

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Running both gateways simultaneously for weeks | Safe rollback | Double RAM (~430 MB wasted), confusion about which is live | First 1-2 weeks of cutover only |
| Hardcoding bridge IDs in iptables scripts | Quick fix | Breaks on every `docker compose down`/`up` cycle | Never -- use `fix-iptables.sh` with dynamic lookup |
| Sharing ContextForge's Postgres | Less RAM, fewer containers | Schema conflicts, migration tool incompatibility | Never |
| Skipping Prometheus/Grafana initially | Faster deployment | No visibility into request latency, error rates, backend health | Acceptable for first deploy, add within 1 week |
| Using `--network=host` for Sentinel | Simplest networking | Bypasses Docker network isolation, all ports exposed on host | Acceptable for single-user VPS, document the tradeoff |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Claude Code MCP config | Changing port number but keeping ContextForge wrapper | Write a new MCP config entry with Sentinel-native wrapper or direct stdio |
| n8n health monitoring | Pointing n8n container-health workflow at ContextForge's container names | Update container-health to check `sentinel-gateway` instead of `mcp-context-forge-gateway-1` |
| Prometheus scraping | Adding Prometheus to a separate Docker network from Sentinel | Prometheus must be on the same Docker network as Sentinel, or Sentinel uses `--network=host` |
| Nightly DB backups | Backup script references only SQLite databases, misses Sentinel's Postgres | Add `pg_dump sentinel` to the backup script alongside existing SQLite backups |
| JWT token | Reusing ContextForge JWT with issuer=`mcpgateway`, audience=`mcpgateway-api` | Generate new JWT with issuer=`sentinel-gateway`, audience=`sentinel-api` matching `sentinel.toml` |
| Grafana data source | Assuming Prometheus is pre-configured | Must add Prometheus as a data source in Grafana, configure scrape target for Sentinel's `/metrics` endpoint |
| n8n webhook for alerts | Creating new workflows instead of updating existing Container Health workflow | Update the existing Container Health workflow (runs every 2 min) to include Sentinel container checks |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Two Postgres instances running | ~200 MB extra RAM, slower disk I/O from double WAL writes | Use separate instances during cutover, decommission ContextForge's after 2 weeks | When VPS RAM drops below 2 GB free |
| Prometheus + Grafana on a 16 GB VPS with 14 containers | Grafana alone uses 100-200 MB, Prometheus uses 50-100 MB depending on retention | Set Prometheus retention to 7 days, Grafana memory limit to 256 MB | When total container RAM exceeds 12 GB |
| Docker image bloat from npm + Playwright in Sentinel image | 400 MB+ image from Chromium, slow builds, slow pulls | Multi-stage build: Rust binary in one stage, npm packages in another, skip Playwright if not actively used | When disk usage exceeds 80% (currently 29%) |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Leaving ContextForge running after cutover with valid JWT | Two gateways accepting the same tool calls, double audit logging, potential inconsistent state | Stop ContextForge gateway (not just redirect traffic) within 1 week of successful cutover |
| Sentinel's Postgres accessible from host at 127.0.0.1:5432 | Other services on localhost can connect to Sentinel's DB | Do not publish Sentinel Postgres port to host -- use Docker-internal networking only (remove `ports:` from compose) |
| Prometheus `/metrics` endpoint exposed without auth | Anyone on localhost can read request rates, tool names, error patterns | Bind Prometheus scrape to Docker-internal network only, not host ports |
| JWT secret shared between ContextForge and Sentinel | Compromising one compromises both; ContextForge may have weaker secret handling | Use different JWT secrets; rotate the Sentinel secret after ContextForge decommission |
| Passing API keys (Firecrawl, Exa) as plain env vars in docker-compose.yml | Keys visible in `docker inspect`, compose file | Use Docker secrets or `.env` file (already the pattern); verify `.env` is in `.gitignore` |

## "Looks Done But Isn't" Checklist

- [ ] **Port binding:** `curl http://127.0.0.1:9200/health` returns Sentinel's response, not ContextForge's -- verify the response body format matches Sentinel (Rust JSON, not Python)
- [ ] **All 7 backends reachable:** Call one tool from EACH backend (n8n, sqlite, context7, firecrawl, exa, sequential-thinking, playwright) -- `tools/list` showing them is not enough
- [ ] **JWT auth works end-to-end:** Claude Code can call `mcp__sentinel-gateway__list_workflows` and get real workflow data back, not just a 200 OK
- [ ] **Audit logging to Postgres:** After a tool call, `SELECT * FROM audit_log ORDER BY created_at DESC LIMIT 1` returns the call -- empty audit table means logging is broken
- [ ] **n8n monitoring sees Sentinel:** The Container Health n8n workflow (every 2 min) includes `sentinel-gateway` in its container list -- missing means no Discord alerts on failure
- [ ] **Prometheus scraping:** `curl http://sentinel:9201/metrics` returns Prometheus-formatted metrics with non-zero counters after some tool calls
- [ ] **Rollback tested:** ContextForge can be restarted and Claude Code can be pointed back at it within 5 minutes -- verify this BEFORE decommissioning
- [ ] **iptables intact:** After all Docker network changes, `iptables -L -v` still shows DROP rules on eth0 for ports 8080 and 9999
- [ ] **Nightly backup updated:** The backup script at `/home/lwb3/backups/nightly-db-backup.sh` includes `pg_dump` for Sentinel's Postgres

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Port collision (Pitfall 1) | LOW | Stop Sentinel, verify port free, fix config, restart |
| MCP config broken (Pitfall 2) | LOW | Revert `~/.claude/settings.json` to ContextForge config, restart Claude Code |
| Network isolation (Pitfall 3) | MEDIUM | Add Sentinel to mcpnet: update compose, `docker compose up -d`, verify DNS |
| Sidecars killed (Pitfall 4) | MEDIUM | Re-create sidecars: `docker compose up -d mcp-n8n mcp-sqlite` in ContextForge dir (or Sentinel dir if migrated) |
| Postgres conflict (Pitfall 5) | HIGH | If schemas corrupted, restore from backup (nightly backup + pg_dump); prevention is far cheaper |
| iptables stale (Pitfall 6) | LOW | Run updated `fix-iptables.sh`, verify with `iptables -L -v` and external port scan |
| stdio backends missing (Pitfall 7) | MEDIUM | Rebuild Docker image with npm packages, or run stdio backends on host outside Docker |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Port collision (1) | Pre-cutover prep | `sentinel.toml`, Dockerfile, and docker-compose.yml all agree on the same port |
| MCP config (2) | Cutover execution | Claude Code successfully calls `tools/list` through new config |
| Network isolation (3) | Pre-cutover prep | `docker exec sentinel-gateway nslookup mcp-n8n` resolves |
| Sidecar ownership (4) | Pre-cutover prep | `docker compose down` in ContextForge dir does NOT stop mcp-n8n or mcp-sqlite |
| Postgres strategy (5) | Pre-cutover prep | Two separate Postgres instances documented, both healthy |
| iptables drift (6) | Post-cutover verify | `iptables -L -v` rules match live Docker bridge IDs |
| stdio in Docker (7) | Deployment | Each of 5 stdio backends responds to at least one tool call |

## Sources

- `/home/lwb3/sentinel-gateway/docker-compose.yml` -- Sentinel compose config showing port 9201 mapping (HIGH confidence, local)
- `/home/lwb3/sentinel-gateway/sentinel.toml` -- Gateway config showing port 9200 listen address (HIGH confidence, local)
- `/home/lwb3/sentinel-gateway/Dockerfile` -- Shows EXPOSE 9201 and healthcheck on 9201 (HIGH confidence, local)
- `/home/lwb3/sentinel-gateway/docs/CURRENT-INFRASTRUCTURE.md` -- Full VPS container map, network topology, 14 containers (HIGH confidence, local)
- `/home/lwb3/sentinel-gateway/docs/CONTEXTFORGE-GATEWAY.md` -- ContextForge deployment details, JWT config, tool catalog (HIGH confidence, local)
- `/home/lwb3/sentinel-gateway/docs/MCP-TOPOLOGY.md` -- Docker network topology, mcpnet/n8nnet/webproxy, wrapper auth flow (HIGH confidence, local)
- `/home/lwb3/mcp-context-forge/docker-compose.slim.yml` -- ContextForge slim overrides, port 9200, Postgres 5434 (HIGH confidence, local)
- `/home/lwb3/v1be-code-server/fix-iptables.sh` -- Known bridge ID drift issue documentation (HIGH confidence, local)
- MEMORY.md -- VPS audit results, iptables cleanup history, backup configuration (HIGH confidence, local)

---
*Pitfalls research for: Sentinel Gateway v1.1 Deploy & Harden*
*Researched: 2026-02-22*
