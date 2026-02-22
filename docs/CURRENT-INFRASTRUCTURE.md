# Current Infrastructure

Complete map of the MCP infrastructure running on the VPS as of 2026-02-22.

## Server Environment

| Component | Value |
|-----------|-------|
| OS | Ubuntu 24.04.4 LTS |
| Kernel | 6.8.0-100-generic |
| CPU | 4 cores |
| RAM | 16 GB |
| Disk | 193 GB (29% used, 56 GB) |
| Docker | 29.2.1 |
| Rust | 1.93.1 (rustc + cargo via rustup) |

## Running Containers (14 total)

### Sentinel Gateway Stack (5 containers)

| Container | Image | Status | Port | RAM |
|-----------|-------|--------|------|-----|
| `mcp-context-forge-gateway-1` | `ghcr.io/ibm/mcp-context-forge:latest` | Healthy | `127.0.0.1:9200 -> 4444` | ~200 MB |
| `mcp-context-forge-postgres-1` | `postgres:18` | Healthy | `5432` (internal) | ~100 MB |
| `mcp-context-forge-redis-1` | `redis:latest` | Up | `6379` (internal) | ~30 MB |
| `mcp-n8n` | `mcp-servers-n8n-mcp` | Healthy | `3000` (internal) | ~50 MB |
| `mcp-sqlite` | `mcp-servers-sqlite-mcp` | Healthy | `3000` (internal) | ~50 MB |

**Subtotal: ~430 MB RAM, 330 MB disk**

### Web Services (4 containers)

| Container | Image | Port | Purpose |
|-----------|-------|------|---------|
| `caddy` | `v1be-code-server-caddy` | `0.0.0.0:80,443` | Reverse proxy + TLS |
| `wallybrain-music` | `wallybrain-music-wallybrain-music` | `127.0.0.1:8800` | Music platform |
| `solitaire` | `solitaire-solitaire` | `127.0.0.1:8801` | Klondike solitaire |
| `authelia` | `authelia/authelia:latest` | `9091` (internal) | 2FA authentication |

### Automation & AI (2 containers)

| Container | Image | Port | Purpose |
|-----------|-------|------|---------|
| `n8n` | `n8nio/n8n:latest` | `127.0.0.1:8567 -> 5678` | Workflow automation |
| `chiasm` | `chiasm-chiasm` | internal | AI companion (Discord + Claude API) |

### Monitoring (3 containers)

| Container | Image | Port | Purpose |
|-----------|-------|------|---------|
| `cpu-server` | `node:20-slim` | internal `:9999` | CPU usage endpoint |
| `system-stats` | `node:20-alpine` | internal `:9998` | Memory/disk endpoint |
| `container-health` | `node:20-alpine` | internal `:9997` | Docker container status |

## Network Architecture

### Port Binding Strategy

Only Caddy binds to `0.0.0.0` (all interfaces). Everything else is `127.0.0.1` (localhost only) or Docker-internal.

| Service | Binding | External Access |
|---------|---------|-----------------|
| Caddy | `0.0.0.0:80,443` | Direct (reverse proxy) |
| All others | `127.0.0.1:port` | Via Caddy only |
| Monitoring | Docker-internal only | Via n8n Docker network |

### iptables Rules

```
DROP eth0 -> 8080   (code-server, Caddy proxies)
DROP eth0 -> 9999   (cpu-server, internal only)
ACCEPT bridge -> 8080 (Docker internal traffic)
```

### Docker Networks

| Network | Services | Purpose |
|---------|----------|---------|
| `mcpnet` | gateway, postgres, redis, mcp-n8n, mcp-sqlite | ContextForge internal |
| `n8nnet` | n8n, mcp-n8n, system-stats, container-health, cpu-server | n8n + monitoring |
| `webproxy` | caddy, wallybrain-music, solitaire, authelia, n8n | Web services |

## Security Posture

| Control | Status |
|---------|--------|
| SSH | Key-only, no root |
| TLS | Auto-renewed by Caddy (wallyblanchard.com, wallybrain.icu) |
| Auth | Authelia TOTP 2FA for all web services |
| Firewall | iptables DROP on eth0 for internal ports |
| Secrets | `.env` files, never committed, pre-commit hook blocks |
| Audit | auditd enabled, Lynis 79/100, Mozilla Observatory A+ |
| Backups | Nightly SQLite backups at 4 AM UTC, 7-day rotation |
| Docker | Log rotation: 10 MB max, 3 files per container |

## Disk Usage

| Directory | Size | Notes |
|-----------|------|-------|
| Docker images | 17.8 GB | 16.5 GB reclaimable (unused images) |
| Docker containers | 3.3 GB | All active |
| Docker build cache | 9.1 GB | 4.1 GB reclaimable |
| `wallybrain-music/` | 8.0 GB | Audio files |
| `mcp-context-forge/` | 331 MB | Sentinel Gateway (ContextForge) |
| `backups/` | 340 MB | Nightly DB backups |
