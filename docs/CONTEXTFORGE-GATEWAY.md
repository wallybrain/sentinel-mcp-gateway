# ContextForge Gateway Deployment (Legacy)

> **Status: REPLACED** — IBM ContextForge was replaced by Sentinel Gateway on 2026-02-22.
> Containers are stopped but preserved for rollback. This document is retained for reference.
>
> **Rollback:** `docker compose -f /home/lwb3/mcp-context-forge/docker-compose.yml start`

IBM ContextForge (Python/FastAPI) deployment details — the system that was replaced by Sentinel Gateway.

## Deployment

**Location:** `/home/lwb3/mcp-context-forge/`
**Launch command:**
```bash
cd /home/lwb3/mcp-context-forge
docker compose -f docker-compose.yml -f docker-compose.slim.yml up -d
```

The `docker-compose.slim.yml` overrides the enterprise defaults for single-user VPS deployment.

## Services

### Gateway (FastAPI + Gunicorn)

| Property | Slim (Production) | Main (Enterprise) |
|----------|-------------------|-------------------|
| Image | `ghcr.io/ibm/mcp-context-forge:latest` |  same |
| Replicas | 1 | 3 |
| Workers | 2 | 24 |
| Port | `127.0.0.1:9200 -> 4444` | same |
| RAM limit | 512 MB | unbounded |
| Transport | StreamableHTTP (SSE + HTTP) | same |

### PostgreSQL 18

| Property | Slim | Main |
|----------|------|------|
| Port | `127.0.0.1:5434` | same |
| Pool size | 5 + 5 overflow | via PgBouncer (600 default + 150 reserve) |
| max_connections | 50 | 800 |
| RAM limit | 512 MB | unbounded |

### Redis

| Property | Slim | Main |
|----------|------|------|
| maxmemory | 128 MB | 1 GB |
| Eviction | allkeys-lru | same |
| RAM limit | 192 MB | unbounded |
| Port | internal only | same |

### PgBouncer (Main Only)

Disabled in slim mode. Transaction-mode pooling with 600 default connections, 150 reserve.

### Nginx Cache (Main Only)

Disabled in slim mode. CDN-like caching reverse proxy.

## Authentication

| Setting | Value |
|---------|-------|
| Algorithm | HS256 |
| Issuer | `mcpgateway` |
| Audience | `mcpgateway-api` |
| Auth required | Yes |
| Token expiration required | Yes |
| JTI (revocation) required | Yes |
| Admin email | `admin@wallybrain.net` |
| Token expiry | February 2027 |

### RBAC Roles

| Role | Scope | Permissions |
|------|-------|-------------|
| `platform_admin` | Global | `*` (everything) |
| `team_admin` | Team | teams.*, tools.read/execute, resources.read |
| `developer` | Team | tools.read/execute, resources.read |
| `viewer` | Team | tools.read, resources.read (read-only) |

## What We Actually Used vs What Was Available

### Used

- Token-based auth (single JWT, admin scope)
- Request routing to 2 backends via virtual server
- Tool discovery (19 tools)
- Health checks
- Audit trail logging
- PostgreSQL state persistence

### Available But Unused

- RBAC (single user)
- Multi-tenancy
- Plugin system (40+ plugins)
- OPA policy engine
- OpenTelemetry tracing
- PgBouncer connection pooling
- Nginx caching layer
- A2A protocol support
- Model/provider proxying
- Canary/blue-green deployments

## Why It Was Replaced

ContextForge used **5 containers (~1 GB RAM, ~330 MB disk)** for what amounted to routing requests to 2 backends with a token check. Sentinel Gateway replaces this with a single ~14 MB Rust binary using <50 MB RAM, while also governing the 5 previously-ungoverned stdio MCP servers.
