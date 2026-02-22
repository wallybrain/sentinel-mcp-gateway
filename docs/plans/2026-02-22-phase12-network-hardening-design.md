# Phase 12: Network Hardening — Design

**Date**: 2026-02-22
**Status**: Approved

## Context

Sentinel Gateway runs as a stdio subprocess (not a network daemon). Traditional network hardening (TLS termination, WAF, reverse proxy) doesn't apply. The actual attack surface is:

1. Health/metrics endpoint on `127.0.0.1:9201` (HTTP, no auth)
2. HTTP backends on localhost (no mutual auth)
3. Docker sidecar network (`sentinelnet` bridge)

## Design

### Layer 1: Docker Container Hardening

Changes to `docker-compose.yml`:

**All services** (postgres, mcp-n8n, mcp-sqlite):
- `security_opt: [no-new-privileges:true]`
- `cap_drop: [ALL]`
- `pids_limit: 100`

**postgres**:
- `cap_add: [SETUID, SETGID]` (postgres user switching)
- `read_only: true` + `tmpfs: [/tmp, /run/postgresql]`
- `mem_limit: 256M`

**mcp-n8n / mcp-sqlite**:
- `read_only: true` + `tmpfs: [/tmp]`
- Already have `memory: 128M`

**sentinelnet network**:
- `internal: true` — no internet access (containers don't need it)

### Layer 2: Health Endpoint Auth

- New env var: `HEALTH_TOKEN`
- `/health` — no auth (liveness probes)
- `/ready` — no auth (readiness probes)
- `/metrics` — requires `Authorization: Bearer <HEALTH_TOKEN>`
- ~20 lines in `src/health/server.rs`

### Layer 3: Backend Shared Secret

**Sentinel (Rust)**:
- New env var: `BACKEND_SHARED_SECRET`
- Add `X-Sentinel-Auth` header to HTTP backend requests
- ~5 lines in `src/backend/http.rs`

**Sidecars (Node.js)**:
- Express middleware validates `X-Sentinel-Auth` header
- Exempt `/health` from auth (Docker healthchecks)
- Pass `BACKEND_SHARED_SECRET` env via docker-compose
- ~10 lines per sidecar (`n8n-mcp-server/index.js`, `sqlite-mcp-server/index.js`)

## Non-Goals

- mTLS (overkill for localhost)
- TLS on health endpoint (localhost-only)
- IPv6 support
- Rate limiting on health endpoint
