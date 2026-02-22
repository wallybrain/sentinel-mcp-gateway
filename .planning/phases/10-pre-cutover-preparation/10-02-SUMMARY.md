# Plan 10-02 Summary: Build and start Sentinel stack alongside ContextForge

**Status:** Complete
**Commit:** bdf4e39

## What Was Done

1. **Stopped old sidecar containers** — `mcp-n8n` and `mcp-sqlite` from mcp-servers compose stopped and removed (name conflict required `docker rm`).

2. **Built and started Sentinel stack** — All 4 containers running and healthy:
   - `sentinel-gateway` — Rust binary, ports 9202→9200 (MCP) and 9201→9201 (health)
   - `sentinel-postgres` — PostgreSQL 16 Alpine, pgdata volume
   - `mcp-n8n` — Node 20 Alpine, HTTP transport sidecar (sentinelnet + n8nnet)
   - `mcp-sqlite` — Node 20 Alpine, HTTP transport sidecar (sentinelnet)

3. **Verified health and connectivity**:
   - Sentinel health: `{"status":"ok"}` on port 9201
   - ContextForge: `{"status":"healthy"}` on port 9200 (no collision)
   - Prometheus metrics: both backends healthy
   - DNS resolution: mcp-n8n (172.20.0.2), mcp-sqlite (172.20.0.3)
   - Gateway→sidecar health: both respond over sentinelnet

4. **ContextForge MCP tools verified** — sqlite_databases, sqlite_tables, sqlite_query all work through ContextForge on port 9200.

## Issues Discovered and Fixed

| Issue | Root Cause | Fix |
|-------|-----------|-----|
| Rust build failed (edition2024) | Dockerfile pinned rust:1.83, needs 1.85+ | Changed to `rust:slim-bookworm` |
| Config permission denied | COPY creates /etc/sentinel/ as root-only | Added `mkdir -p` + `chmod 755` before COPY |
| stdio backends fail (no node) | Container has no Node.js runtime | Created `sentinel-docker.toml` with HTTP-only backends |
| Listen on 127.0.0.1 inside Docker | Port mapping can't reach loopback | Docker config uses `0.0.0.0` binding |
| JWT InvalidSignature | Rust uses `secret.as_bytes()` (raw string), not base64-decoded | Regenerated token with PyJWT using raw string key |
| Container name conflict | Old mcp-n8n/mcp-sqlite containers still existed (stopped, not removed) | `docker rm` before `compose up` |

## Requirements Satisfied

| Requirement | Status |
|-------------|--------|
| PREP-05 | Sentinel containers start and pass health checks alongside ContextForge |

## Human Verification

User confirmed all checks pass and ContextForge MCP tools still work.
