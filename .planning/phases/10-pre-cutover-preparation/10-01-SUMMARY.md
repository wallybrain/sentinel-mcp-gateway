# Plan 10-01 Summary: Port fix, sidecar migration, Docker networking, production .env

**Status:** Complete
**Commit:** f953279

## What Was Done

1. **Dockerfile port fix** — Added `EXPOSE 9200` (MCP transport) alongside existing `EXPOSE 9201` (health). Resolves PREP-01 config drift.

2. **Sidecar Dockerfiles copied** — Created `sidecars/Dockerfile.n8n` and `sidecars/Dockerfile.mcp-sqlite` (renamed from `.sqlite` to avoid `.gitignore` `*.sqlite` pattern). Contents match source files in `/home/lwb3/mcp-servers/` exactly. Satisfies PREP-02.

3. **docker-compose.yml rewritten** — Full 4-service stack:
   - `gateway`: temporary `127.0.0.1:9202:9200` (avoids collision with ContextForge on 9200)
   - `postgres`: unchanged, added to sentinelnet
   - `mcp-n8n`: HTTP transport sidecar, dual-network (sentinelnet + n8nnet external)
   - `mcp-sqlite`: HTTP transport sidecar, sentinelnet only, volume mount for databases
   - Networks: `sentinelnet` (bridge) + `n8nnet` (external: `n8n-mcp_default`)
   - Satisfies PREP-02, PREP-03.

4. **Production .env created** — JWT_SECRET_KEY (generated), POSTGRES_PASSWORD (generated), N8N_API_KEY (extracted from running container). File is gitignored. `.env.example` updated with N8N_API_KEY placeholder. Satisfies PREP-04.

## Verification

- `docker compose config` resolves without errors
- Both ports exposed in Dockerfile
- 5 sentinelnet references in docker-compose.yml
- .env not staged/committed
- Sidecar Dockerfiles match source exactly

## Requirements Satisfied

| Requirement | Status |
|-------------|--------|
| PREP-01 | Port config consistent across sentinel.toml, Dockerfile, docker-compose.yml |
| PREP-02 | Sidecar definitions migrated to Sentinel compose |
| PREP-03 | sentinelnet network defined, all services joined |
| PREP-04 | Production .env exists with real secrets |
