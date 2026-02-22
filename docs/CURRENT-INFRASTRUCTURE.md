# Current Infrastructure

Complete map of the MCP infrastructure running on the VPS as of 2026-02-22.

## Server Environment

| Component | Value |
|-----------|-------|
| OS | Ubuntu 24.04.4 LTS |
| Kernel | 6.8.0-100-generic |
| CPU | 4 cores |
| RAM | 16 GB |
| Disk | 193 GB (43% used, 82 GB) |
| Docker | 29.2.1 |
| Rust | 1.93.1 (rustc + cargo via rustup) |
| Node.js | v20.20.0 (nvm) |

## MCP Gateway

Sentinel Gateway runs as a **native binary** (`target/release/sentinel-gateway`, ~14 MB), spawned by Claude Code via stdio transport. It is **not** a Docker container.

| Component | Details |
|-----------|---------|
| Binary | `/home/lwb3/sentinel-gateway/target/release/sentinel-gateway` |
| Transport | stdio (stdin/stdout with Claude Code) |
| Config | `sentinel.toml` |
| Health endpoint | `127.0.0.1:9201` |
| Auth | Session-level JWT |
| Predecessor | IBM ContextForge (stopped, containers preserved for rollback) |

### Managed Backends

| Backend | Type | Address | Purpose |
|---------|------|---------|---------|
| mcp-n8n | HTTP | `127.0.0.1:3001` | n8n workflow API bridge |
| mcp-sqlite | HTTP | `127.0.0.1:3002` | SQLite database operations |
| context7 | stdio | managed child process | Library documentation lookup |
| firecrawl | stdio | managed child process | Web scraping/crawling |
| playwright | stdio | managed child process | Browser automation |
| sequential-thinking | stdio | managed child process | Chain-of-thought reasoning |
| ollama | stdio | managed child process (disabled) | Local LLM inference |
| exa | stdio | disabled in config | Web search (needs EXA_API_KEY) |

## Running Containers (12 total)

### Sentinel Sidecar Stack (3 containers)

| Container | Image | Status | Port | RAM |
|-----------|-------|--------|------|-----|
| `sentinel-postgres` | `postgres:16-alpine` | Healthy | `127.0.0.1:5432` | ~26 MB |
| `mcp-n8n` | `sentinel-gateway-mcp-n8n` | Healthy | `127.0.0.1:3001` | ~36 MB |
| `mcp-sqlite` | `sentinel-gateway-mcp-sqlite` | Healthy | `127.0.0.1:3002` | ~34 MB |

**Compose file:** `/home/lwb3/sentinel-gateway/docker-compose.yml`

### Web Services (4 containers)

| Container | Image | Port | RAM | Purpose |
|-----------|-------|------|-----|---------|
| `caddy` | `v1be-code-server-caddy` | `0.0.0.0:80,443` | ~24 MB | Reverse proxy + TLS |
| `wallybrain-music` | `wallybrain-music-wallybrain-music` | `127.0.0.1:8800` | ~49 MB | Music platform (wallybrain.net) |
| `solitaire` | `solitaire-solitaire` | `127.0.0.1:8801` | ~11 MB | Klondike solitaire |
| `authelia` | `authelia/authelia:latest` | `9091` (internal) | ~23 MB | 2FA authentication |

### Automation & AI (2 containers)

| Container | Image | Port | RAM | Purpose |
|-----------|-------|------|-----|---------|
| `n8n` | `n8nio/n8n:latest` | `127.0.0.1:8567` | ~319 MB | Workflow automation |
| `chiasm` | `chiasm-chiasm` | internal | ~77 MB | AI companion (Discord + Claude API) |

### Monitoring (3 containers)

| Container | Image | Port | RAM | Purpose |
|-----------|-------|------|-----|---------|
| `cpu-server` | `node:20-slim` | internal `:9999` | ~12 MB | CPU usage endpoint |
| `system-stats` | `node:20-alpine` | internal `:9998` | ~8 MB | Memory/disk endpoint |
| `container-health` | `node:20-alpine` | internal `:9997` | ~7 MB | Docker container status |

### Stopped Containers (ContextForge — preserved for rollback)

| Container | Image | Notes |
|-----------|-------|-------|
| `mcp-context-forge-gateway-1` | `ghcr.io/ibm/mcp-context-forge:latest` | Was on `127.0.0.1:9200` |
| `mcp-context-forge-postgres-1` | `postgres:18` | Was on `5433` (internal) |
| `mcp-context-forge-redis-1` | `redis:latest` | Was on `6379` (internal) |
| `mcp-context-forge-pgbouncer-1` | `edoburu/pgbouncer:latest` | Was on `6432` (internal) |
| `mcp-context-forge-fast_time_server-1` | `ghcr.io/ibm/fast-time-server:latest` | Was on `8888` (internal) |

**Compose file:** `/home/lwb3/mcp-context-forge/docker-compose.yml`
**Rollback:** `docker compose -f /home/lwb3/mcp-context-forge/docker-compose.yml start`

## Network Architecture

### Port Binding Strategy

Only Caddy and SSH bind to `0.0.0.0` (all interfaces). Everything else is `127.0.0.1` (localhost only) or Docker-internal.

| Service | Binding | External Access |
|---------|---------|-----------------|
| SSH | `0.0.0.0:22` | Direct |
| Caddy | `0.0.0.0:80,443` | Direct (reverse proxy) |
| code-server | `0.0.0.0:8080` | Via Caddy + iptables DROP on eth0 |
| Sentinel health | `127.0.0.1:9201` | None (localhost only) |
| All other services | `127.0.0.1:port` | Via Caddy only |
| Monitoring | Docker-internal only | Via n8n Docker network |

### iptables Rules

```
ACCEPT bridge(br-03b170a2a124) -> 8080  (Docker internal → code-server)
DROP   eth0 -> 8080                      (block external → code-server)
DROP   eth0 -> 9999                      (block external → cpu-server)
```

**Managed by:** `/home/lwb3/v1be-code-server/fix-iptables.sh` (idempotent, uses privileged Docker container)
**Note:** Bridge name `br-03b170a2a124` changes if webproxy network is recreated.

### Docker Networks

| Network | Services | Purpose |
|---------|----------|---------|
| `sentinel-gateway_sentinelnet` | sentinel-postgres, mcp-n8n, mcp-sqlite | Sentinel sidecar internal |
| `n8n-mcp_default` | n8n, mcp-n8n, chiasm, cpu-server, system-stats, container-health | n8n + monitoring |
| `webproxy` | caddy, wallybrain-music, solitaire, authelia, chiasm | Web services |
| `mcp-context-forge_mcpnet` | (empty — all containers stopped) | **Stale, safe to remove** |

## Security Posture

| Control | Status |
|---------|--------|
| SSH | Key-only, no root |
| TLS | Auto-renewed by Caddy (wallyblanchard.com, wallybrain.icu, wallybrain.net) |
| Auth | Authelia TOTP 2FA for all web services |
| Firewall | iptables DROP on eth0 for internal ports |
| Secrets | `.env` files, never committed, global pre-commit hook blocks |
| Audit | auditd enabled, Lynis 79/100, Mozilla Observatory A+ |
| Backups | Nightly SQLite backups at 4 AM UTC, 7-day rotation |
| Docker | Log rotation: 10 MB max, 3 files per container |
| MCP Gateway | JWT auth, rate limiting, circuit breakers, audit logging (Sentinel) |
