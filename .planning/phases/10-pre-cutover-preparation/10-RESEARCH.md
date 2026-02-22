# Phase 10 Research: Pre-Cutover Preparation

**Phase Goal:** Sentinel containers run healthy alongside ContextForge with all config drift resolved and sidecars migrated
**Requirements:** PREP-01, PREP-02, PREP-03, PREP-04, PREP-05
**Researched:** 2026-02-22

## Summary

Phase 10 is infrastructure preparation -- no Rust code changes. The work is: fix port config drift across three files, migrate two sidecar service definitions from a separate compose project into Sentinel's compose, wire up Docker networking so Sentinel can reach those sidecars by hostname, create a production .env file, and verify Sentinel starts healthy while ContextForge still occupies port 9200. Every task is mechanical and verifiable with a single curl command or docker inspect.

The primary complexity is understanding where everything currently lives. The sidecars (`mcp-n8n`, `mcp-sqlite`) are NOT defined in ContextForge's docker-compose -- they are in a separate compose project at `/home/lwb3/mcp-servers/docker-compose.yml`. This means `docker compose down` in the ContextForge directory does NOT kill the sidecars (contrary to what the pitfalls research assumed). However, the sidecars currently join `mcp-context-forge_mcpnet` as an external network. If that network is destroyed during ContextForge teardown, the sidecars lose DNS resolution. The migration must either: (a) move sidecar definitions into Sentinel's compose with a new network, or (b) keep the sidecars in their current compose but point them at a Sentinel-owned network.

## Requirement Analysis

### PREP-01: Port Config Consistency

**Current state (verified):**

| File | Port Reference | What It Does |
|------|---------------|--------------|
| `sentinel.toml` line 6 | `listen = "127.0.0.1:9200"` | Rust binary binds MCP transport to this address |
| `sentinel.toml` (absent) | `health_listen` not set | Defaults to `127.0.0.1:9201` (hardcoded in `src/config/types.rs:165`) |
| `Dockerfile` line 27 | `EXPOSE 9201` | Documents the health port (not the MCP port) |
| `Dockerfile` line 30 | `curl -sf http://127.0.0.1:9201/health` | Healthcheck targets health port |
| `docker-compose.yml` line 15 | `127.0.0.1:9201:9201` | Maps ONLY the health port to host |

**The drift:** The Rust binary listens on TWO ports -- 9200 (MCP transport) and 9201 (health/metrics). The Dockerfile only EXPOSEs 9201 and healthchecks against 9201. The docker-compose only maps 9201 to the host. Port 9200 (the actual MCP traffic port) is NOT mapped to the host at all.

This is fine for development (tests hit the binary directly), but for production deployment, Claude Code needs to reach port 9200 on the host. The Dockerfile should EXPOSE both ports, and docker-compose must map both.

**Resolution strategy:**

1. `sentinel.toml` -- no change needed. `listen = "127.0.0.1:9200"` and `health_listen` defaults to `127.0.0.1:9201`. Both correct.
2. `Dockerfile` -- add `EXPOSE 9200` alongside existing `EXPOSE 9201`. Keep healthcheck on 9201.
3. `docker-compose.yml` -- add `127.0.0.1:9200:9200` port mapping. BUT during Phase 10, ContextForge occupies host port 9200. So initially map to a TEMPORARY port: `127.0.0.1:9202:9200` for Phase 10 testing. Phase 11 (cutover) will rebind to 9200 after ContextForge stops.

**Alternative:** Change `sentinel.toml` to listen on 9201 for MCP traffic during Phase 10, avoiding the temporary port. Rejected -- the binary has two separate listeners (MCP on `gateway.listen`, health on `gateway.health_listen`). Changing the MCP port creates a different config from what will run in production. Better to use a host-side port remap.

**Verification:** `curl -sf http://127.0.0.1:9202/health` returns 200 (proves MCP port is reachable). `curl -sf http://127.0.0.1:9201/health` returns 200 (proves health port is reachable). Both must return Sentinel's response format, not ContextForge's.

### PREP-02: Sidecar Migration

**Current ownership (verified via `docker inspect`):**

| Container | Compose Project | Compose File | Image |
|-----------|----------------|--------------|-------|
| `mcp-n8n` | `mcp-servers` | `/home/lwb3/mcp-servers/docker-compose.yml` | `mcp-servers-n8n-mcp` (built from `/home/lwb3/n8n-mcp-server` + `Dockerfile.http`) |
| `mcp-sqlite` | `mcp-servers` | `/home/lwb3/mcp-servers/docker-compose.yml` | `mcp-servers-sqlite-mcp` (built from `/home/lwb3/sqlite-mcp-server` + `Dockerfile.sqlite-http`) |

**Key finding:** The sidecars are NOT in ContextForge's compose. They are in a separate `mcp-servers` project. This is less risky than Pitfall 4 assumed -- `docker compose down` in the ContextForge directory will NOT kill these containers. However, they currently join `mcp-context-forge_mcpnet` as an external network.

**Sidecar service definitions to migrate:**

```yaml
# mcp-n8n (from /home/lwb3/mcp-servers/docker-compose.yml)
n8n-mcp:
  build:
    context: /home/lwb3/n8n-mcp-server
    dockerfile: /home/lwb3/mcp-servers/Dockerfile.http
  container_name: mcp-n8n
  restart: unless-stopped
  environment:
    - MCP_TRANSPORT=http
    - MCP_PORT=3000
    - N8N_URL=http://n8n:5678
    - N8N_API_KEY=${N8N_API_KEY}
  healthcheck:
    test: ["CMD", "wget", "-q", "--spider", "http://localhost:3000/health"]
    interval: 30s
    timeout: 5s
    retries: 3
    start_period: 10s
  networks:
    - mcpnet   # currently mcp-context-forge_mcpnet
    - n8nnet   # currently n8n-mcp_default
  deploy:
    resources:
      limits:
        memory: 128M
  logging:
    driver: json-file
    options:
      max-size: "10m"
      max-file: "3"
```

```yaml
# mcp-sqlite (from /home/lwb3/mcp-servers/docker-compose.yml)
sqlite-mcp:
  build:
    context: /home/lwb3/sqlite-mcp-server
    dockerfile: /home/lwb3/mcp-servers/Dockerfile.sqlite-http
  container_name: mcp-sqlite
  restart: unless-stopped
  environment:
    - MCP_TRANSPORT=http
    - MCP_PORT=3000
    - SQLITE_DB_DIR=/data
  volumes:
    - /home/lwb3/databases:/data
  healthcheck:
    test: ["CMD", "wget", "-q", "--spider", "http://localhost:3000/health"]
    interval: 30s
    timeout: 5s
    retries: 3
    start_period: 10s
  networks:
    - mcpnet   # currently mcp-context-forge_mcpnet
  deploy:
    resources:
      limits:
        memory: 128M
  logging:
    driver: json-file
    options:
      max-size: "10m"
      max-file: "3"
```

**Dockerfile dependencies:**

The sidecar Dockerfiles are in `/home/lwb3/mcp-servers/`:
- `Dockerfile.http` -- Node 20-alpine, `npm ci`, `node index.js` (8 lines)
- `Dockerfile.sqlite-http` -- Node 20-alpine + python3/make/g++ (native deps), `npm ci`, `node index.js` (9 lines)

The build contexts are:
- `/home/lwb3/n8n-mcp-server` -- contains `index.js`, `package.json`, `package-lock.json`, `node_modules`
- `/home/lwb3/sqlite-mcp-server` -- same structure

**Migration approach:**

Option A (recommended): Move the service definitions into Sentinel's `docker-compose.yml`. Copy the Dockerfiles into Sentinel's repo (or reference them by absolute path). The old `mcp-servers` compose becomes dead and can be removed after verification.

Option B: Keep `mcp-servers/docker-compose.yml` but change its network references from `mcp-context-forge_mcpnet` to a Sentinel-owned network. Less disruption but creates a dependency between two compose projects.

**Recommendation: Option A.** Sentinel should own everything it depends on. Having sidecars in a separate compose project is fragile -- if someone runs `docker compose down` in `/home/lwb3/mcp-servers/`, Sentinel's backends die.

**Migration steps:**
1. Copy `Dockerfile.http` and `Dockerfile.sqlite-http` from `/home/lwb3/mcp-servers/` into Sentinel repo (e.g., `sidecars/` directory)
2. Add both service definitions to Sentinel's `docker-compose.yml` with updated build context paths and network references
3. `docker compose up -d mcp-n8n mcp-sqlite` from Sentinel's compose
4. Verify: `docker exec sentinel-gateway nslookup mcp-n8n` resolves, `curl` from inside gateway to `http://mcp-n8n:3000/health` returns 200
5. Stop old sidecar containers: `docker compose -f /home/lwb3/mcp-servers/docker-compose.yml down`
6. Verify Sentinel-managed sidecars are still running

**Secrets:** `mcp-n8n` needs `N8N_API_KEY`. Currently hardcoded as a JWT in the running container's env. This must go into Sentinel's `.env` file.

### PREP-03: Docker Network Topology

**Current networks (verified via `docker network ls`):**

| Network | ID | Services |
|---------|-----|----------|
| `mcp-context-forge_mcpnet` | `1b16eef06308` | ContextForge gateway, postgres, redis, mcp-n8n, mcp-sqlite |
| `n8n-mcp_default` | `7f56bc64e603` | n8n, mcp-n8n, system-stats, container-health, cpu-server, chiasm |
| `webproxy` | `03b170a2a124` | caddy, wallybrain-music, solitaire, authelia, n8n |

**Target network topology:**

Two options for Sentinel to reach the sidecars:

**Option A: Join existing `mcp-context-forge_mcpnet`**

Pros:
- Zero disruption -- sidecars already on this network with DNS names `mcp-n8n` and `mcp-sqlite`
- `sentinel.toml` already references `http://mcp-n8n:3000` and `http://mcp-sqlite:3000`

Cons:
- Network is owned by the ContextForge compose project. When ContextForge is fully removed in Phase 11, this network may be destroyed if no containers reference it
- Confusing name -- `mcp-context-forge_mcpnet` belongs to a dead project

**Option B: Create new `sentinelnet` network**

Pros:
- Clean ownership -- Sentinel owns its network
- No dependency on ContextForge network surviving

Cons:
- Must recreate sidecars on new network (brief downtime for HTTP backends)
- Must ensure `mcp-n8n` also joins `n8n-mcp_default` (it bridges two networks)

**Recommendation: Option B (new `sentinelnet`).** Since we are migrating sidecar ownership (PREP-02), the sidecars will be recreated anyway. They can join the new network at that point. The old `mcp-context-forge_mcpnet` network can be cleaned up in Phase 12 (NET-04).

**Network definitions needed in Sentinel's docker-compose.yml:**

```yaml
networks:
  sentinelnet:
    driver: bridge
  n8nnet:
    external: true
    name: n8n-mcp_default
```

**Service network assignments:**

| Service | Networks | Reason |
|---------|----------|--------|
| `gateway` | `sentinelnet`, `n8nnet` | Reaches sidecars on sentinelnet; joins n8nnet so n8n health workflows can reach `/health` |
| `postgres` | `sentinelnet` | Only gateway needs DB access |
| `mcp-n8n` | `sentinelnet`, `n8nnet` | Reachable by gateway on sentinelnet; reaches n8n on n8nnet |
| `mcp-sqlite` | `sentinelnet` | Reachable by gateway on sentinelnet |

**Verification:** From inside the gateway container: `nslookup mcp-n8n` resolves. `wget -q --spider http://mcp-n8n:3000/health` succeeds. `nslookup mcp-sqlite` resolves. `wget -q --spider http://mcp-sqlite:3000/health` succeeds.

### PREP-04: Production .env File

**Required variables (from sentinel.toml, docker-compose.yml, and sidecar envs):**

| Variable | Source | Used By |
|----------|--------|---------|
| `JWT_SECRET_KEY` | New (generate) | Gateway JWT validation |
| `POSTGRES_PASSWORD` | New (generate) | Postgres init + Gateway DATABASE_URL |
| `N8N_API_KEY` | Existing (from mcp-n8n container) | mcp-n8n sidecar |

**Variables NOT needed yet (Phase 13+):**
- `GRAFANA_ADMIN_PASSWORD` -- monitoring stack
- `FIRECRAWL_API_KEY` -- stdio backends (launched inside gateway container, keys injected via Dockerfile or compose env)
- `EXA_API_KEY` -- same

**JWT secret considerations:**
- Must be a NEW secret, different from ContextForge's `JWT_SECRET_KEY`
- ContextForge JWT uses issuer=`mcpgateway`, audience=`mcpgateway-api`
- Sentinel JWT uses issuer=`sentinel-gateway`, audience=`sentinel-api` (per `sentinel.toml` lines 16-18)
- Reusing the same secret value is safe (different iss/aud claims prevent cross-acceptance) but using a different secret is better practice

**Generation:**
```bash
# JWT secret (64 bytes, base64)
openssl rand -base64 64 | tr -d '\n'

# Postgres password (32 bytes, base64, URL-safe)
openssl rand -base64 32 | tr -d '\n' | tr '+/' '-_'
```

**N8N_API_KEY:** The existing value is a JWT: `eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJmZDM2NjNhZS1jZjYzLTQwNDItYmNmNS03OWQwZDcwNDliZjUiLCJpc3MiOiJuOG4iLCJhdWQiOiJwdWJsaWMtYXBpIiwiaWF0IjoxNzY5ODg1NTQ0fQ.Jp_NgLFFFEuz1qgzOZVdUNEtLmiLxBczWRKXN7t3kZo`. This is an n8n-issued API key, not a secret we generated. It must be copied from the running container or the existing mcp-servers `.env`.

**File location:** `/home/lwb3/sentinel-gateway/.env` (already in `.gitignore`)

**Verification:** `.env` exists, is not committed (`git status` shows it untracked or ignored), `docker compose config` resolves all `${VAR}` references without errors.

### PREP-05: Sentinel Health Alongside ContextForge

**Constraint:** ContextForge is running on `127.0.0.1:9200`. Sentinel must start without conflicting.

**Strategy:** During Phase 10, Sentinel's MCP port maps to a temporary host port:
- `127.0.0.1:9202:9200` -- MCP transport (temporary, will become 9200 in Phase 11)
- `127.0.0.1:9201:9201` -- health/metrics (no conflict, ContextForge doesn't use 9201)

**Verification sequence:**
1. `docker compose up -d` in Sentinel directory
2. `docker ps` shows `sentinel-gateway` with status `healthy`
3. `curl -sf http://127.0.0.1:9201/health` returns 200 with Sentinel's JSON format
4. `curl -sf http://127.0.0.1:9200/health` still returns ContextForge's response (proves no collision)
5. `curl -sf http://127.0.0.1:9202/health` returns 200 from Sentinel's MCP transport listener

**Risk:** If someone accidentally maps Sentinel to 9200 before stopping ContextForge, Docker will either fail the bind (good -- obvious error) or silently succeed if ContextForge was briefly restarted (bad -- traffic goes to wrong service). Always verify response format after `curl`.

## Dependencies and Ordering

```
PREP-01 (port fix) ──> PREP-05 (start containers)
PREP-02 (sidecars) ──> PREP-03 (networking) ──> PREP-05 (start containers)
PREP-04 (.env) ──────> PREP-05 (start containers)
```

PREP-01, PREP-02, and PREP-04 are independent of each other and can be done in parallel. PREP-03 depends on PREP-02 (must know which services join which networks). PREP-05 depends on all four (containers need correct ports, sidecars, networks, and secrets).

**Suggested plan split:**
- **Plan 10-01:** Port fix (PREP-01) + .env creation (PREP-04) + sidecar migration (PREP-02) + network setup (PREP-03) -- all config/file changes, no containers started yet
- **Plan 10-02:** Build and start Sentinel alongside ContextForge (PREP-05) -- docker compose up, verification

## Exact Files to Modify

| File | Change | Requirement |
|------|--------|-------------|
| `Dockerfile` | Add `EXPOSE 9200` | PREP-01 |
| `docker-compose.yml` | Add MCP port mapping, sidecar services, network definitions, env vars for sidecars | PREP-01, PREP-02, PREP-03 |
| `.env` (new) | Create with JWT_SECRET_KEY, POSTGRES_PASSWORD, N8N_API_KEY | PREP-04 |

**Files to copy into repo:**
| Source | Destination | Purpose |
|--------|-------------|---------|
| `/home/lwb3/mcp-servers/Dockerfile.http` | `sidecars/Dockerfile.n8n` | mcp-n8n build |
| `/home/lwb3/mcp-servers/Dockerfile.sqlite-http` | `sidecars/Dockerfile.sqlite` | mcp-sqlite build |

**Files NOT modified:**
- `sentinel.toml` -- listen address (9200) and health_listen default (9201) are already correct
- `src/` -- no Rust code changes in this phase
- `.env.example` -- update with new variables (N8N_API_KEY)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Sidecar recreation causes brief n8n/sqlite downtime | HIGH | LOW | Downtime is seconds; Claude Code retries; no user-facing services affected |
| N8N_API_KEY is wrong/expired | LOW | MEDIUM | Test `curl http://mcp-n8n:3000/health` immediately after sidecar start |
| Old mcp-servers containers conflict with new ones | MEDIUM | LOW | Stop old containers first (`docker compose -f /home/lwb3/mcp-servers/docker-compose.yml down`) before starting Sentinel's |
| ContextForge network destroyed prematurely | LOW | MEDIUM | Option B (new network) avoids this entirely |
| Port 9201 conflict with unknown service | LOW | LOW | Verified no other service uses 9201 on this VPS |

## Open Questions (resolved)

1. **Where are sidecar definitions?** Resolved: `/home/lwb3/mcp-servers/docker-compose.yml`, NOT in ContextForge's compose. This is less risky than originally assumed.

2. **Can sidecars survive ContextForge teardown?** Yes, because they are in a separate compose project. The risk is the shared network (`mcp-context-forge_mcpnet`) being destroyed, not the containers themselves.

3. **What is the actual n8n network name?** Verified: `n8n-mcp_default` (not `n8nnet`). Must use `external: true` with `name: n8n-mcp_default`.

4. **Does Sentinel need `stdin_open: true`?** Yes, the current docker-compose.yml has this. The gateway binary reads JSON-RPC from stdin when used as a stdio transport. Keep it.

## Sources

All findings verified against live infrastructure on 2026-02-22:

- `/home/lwb3/sentinel-gateway/sentinel.toml` -- gateway config, listen 127.0.0.1:9200
- `/home/lwb3/sentinel-gateway/Dockerfile` -- EXPOSE 9201, healthcheck 9201
- `/home/lwb3/sentinel-gateway/docker-compose.yml` -- port 9201 only, no networks
- `/home/lwb3/sentinel-gateway/src/config/types.rs` -- default_listen=9200, default_health_listen=9201
- `/home/lwb3/mcp-servers/docker-compose.yml` -- sidecar definitions, network refs
- `/home/lwb3/mcp-servers/Dockerfile.http` -- mcp-n8n build (node:20-alpine)
- `/home/lwb3/mcp-servers/Dockerfile.sqlite-http` -- mcp-sqlite build (node:20-alpine + native deps)
- `docker inspect mcp-n8n` -- env vars, networks (mcpnet + n8nnet), compose project=mcp-servers
- `docker inspect mcp-sqlite` -- env vars, networks (mcpnet only), volume bind /home/lwb3/databases:/data
- `docker network ls` -- mcpnet=1b16eef06308, n8nnet=7f56bc64e603, webproxy=03b170a2a124

---
*Research completed: 2026-02-22*
*Ready for planning: yes*
