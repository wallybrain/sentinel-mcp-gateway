# ContextForge Gateway Deployment

IBM ContextForge (Python/FastAPI) deployment details — the system being replaced.

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

### Team Scoping

```
JWT 'teams' claim:
  Missing        -> public-only (secure default)
  null + is_admin -> admin bypass (current setup)
  ["team-id"]    -> team + public resources
```

## Enabled Features

| Feature | Status | Notes |
|---------|--------|-------|
| Audit trail | Enabled | Every API request logged to Postgres |
| Security logging | Enabled | All levels |
| DB metrics recording | Enabled | Performance metrics to DB |
| Permission audit | Enabled | RBAC decision logging |
| Structured DB logging | Enabled | Queryable log records |
| Admin UI | Enabled | Web interface |
| Admin API | Enabled | REST management API |
| Catalog | Enabled | MCP server catalog (1092-line YAML) |
| Plugins | Enabled | 40+ available |

## Tool Configuration

| Setting | Value |
|---------|-------|
| Tool timeout | 60s |
| Max retries | 3 |
| Rate limit | 1000 req/s |
| Concurrent limit | 50 |
| Health check interval | 300s |
| Unhealthy threshold | 3 failures |

## Virtual Server

**Combined Server ID:** `7b99a944e63845d6bb87b6d5fa3cdf87`

This single virtual server routes to both backends, presenting a unified tool catalog of 19 tools.

### n8n Backend (9 tools)

| Tool | Purpose |
|------|---------|
| `list_workflows` | List all n8n workflows |
| `get_workflow` | Get workflow details |
| `create_workflow` | Create new workflow |
| `update_workflow` | Update existing workflow |
| `delete_workflow` | Delete workflow |
| `activate_workflow` | Activate/deactivate |
| `execute_workflow` | Run workflow manually |
| `list_executions` | Execution history |
| `get_execution` | Execution details |

### SQLite Backend (10 tools)

| Tool | Purpose |
|------|---------|
| `sqlite_query` | Run SELECT queries |
| `sqlite_execute` | Run INSERT/UPDATE/DELETE/DDL |
| `sqlite_tables` | List tables in database |
| `sqlite_schema` | Get table schema |
| `sqlite_describe` | Full database description |
| `sqlite_create_table` | Create new table |
| `sqlite_insert` | Insert rows |
| `sqlite_backup` | Backup database |
| `sqlite_analyze` | Analyze database |
| `sqlite_databases` | List available databases |

## Environment Variables (Key Names)

Secrets live in `/home/lwb3/mcp-context-forge/.env` (never committed).

| Variable | Purpose |
|----------|---------|
| `POSTGRES_PASSWORD` | Database auth |
| `JWT_SECRET_KEY` | Token signing |
| `AUTH_ENCRYPTION_SECRET` | Encryption key |
| `PLATFORM_ADMIN_PASSWORD` | Admin account |
| `PLATFORM_ADMIN_EMAIL` | Admin identity |
| `LOG_LEVEL` | Gateway logging |
| `AUDIT_TRAIL_ENABLED` | Audit on/off |

## What We Actually Use vs What's Available

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

## Health Check

```python
# Gateway health endpoint
GET http://127.0.0.1:9200/health
# Returns: {"status": "healthy", ...}

# Docker healthcheck runs every 30s with 10s timeout
```
