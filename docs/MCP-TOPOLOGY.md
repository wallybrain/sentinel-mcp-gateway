# MCP Topology

How AI agents connect to MCP servers through Sentinel Gateway.

## Connection Overview

All MCP traffic flows through a single governed chokepoint — Sentinel Gateway.

### Governed (via Sentinel Gateway)

```
Claude Code (local, stdio)
    |
    | stdio (JSON-RPC over stdin/stdout)
    |
    v
OpenClaw (remote, SSH tunnel)
    |
    | mcporter → SSH → run-sentinel.sh → stdio
    | (WireGuard encrypted, cross-architecture)
    |
    v
Sentinel Gateway (native Rust binary, ~14 MB)
    |  JWT auth, rate limiting, circuit breakers, audit logging
    |
    +---> HTTP Backends
    |     +---> mcp-n8n (127.0.0.1:3001, Docker sentinelnet)
    |     |       +---> n8n:5678 (Docker n8n-mcp_default network)
    |     |             7 workflows (6 active monitoring + 1 inactive)
    |     |
    |     +---> mcp-sqlite (127.0.0.1:3002, Docker sentinelnet)
    |             +---> /home/lwb3/databases/*.db (volume mount)
    |
    +---> Managed stdio Backends (child processes)
          +---> context7 (library documentation)
          +---> firecrawl (web scraping)
          +---> playwright (browser automation)
          +---> sequential-thinking (chain-of-thought)
          +---> ollama (local LLM, disabled)
```

**Auth flow (local — Claude Code):**
1. Claude Code spawns Sentinel binary as stdio subprocess
2. Sentinel reads JWT from `SENTINEL_TOKEN` env var, validates at startup
3. All tool calls are authenticated, rate-limited, and audit-logged
4. HTTP backends reached via reqwest; stdio backends managed as child processes

**Auth flow (remote — OpenClaw via SSH tunnel):**
1. OpenClaw agent invokes mcporter skill: `mcporter call sentinel.<tool>`
2. mcporter SSHs to the backend host, runs `run-sentinel.sh`
3. `run-sentinel.sh` sources `.env`, exports secrets, execs sentinel binary
4. Sentinel performs JWT auth, RBAC, rate limiting, audit logging
5. Tool call routed to backend, response flows back through SSH tunnel
6. Secrets never leave the backend host — only JSON-RPC messages traverse the tunnel
5. Circuit breakers isolate failing backends automatically

### Previously Ungoverned (now governed)

These servers were previously launched directly by Claude Code with no auth/audit. They are now managed by Sentinel as stdio child processes with full governance:

| Server | Status | Notes |
|--------|--------|-------|
| `context7` | Governed | Managed stdio backend |
| `firecrawl` | Governed | Requires `FIRECRAWL_API_KEY` (inherited from `.env`) |
| `playwright` | Governed | Headless Chromium, `--no-sandbox` |
| `sequential-thinking` | Governed | Chain-of-thought reasoning |
| `exa` | Disabled | Needs `EXA_API_KEY` (commented out in sentinel.toml) |

## Data Flow Examples

### Claude Code -> n8n Workflow List

```
1. Claude Code calls mcp__sentinel-gateway__list_workflows()
2. Sentinel serializes JSON-RPC request
3. Sentinel POSTs to http://127.0.0.1:3001 (mcp-n8n HTTP backend)
4. Sentinel validates JWT, checks rate limits, logs audit trail
5. mcp-n8n queries n8n API: GET http://n8n:5678/api/v1/workflows
6. Response flows back: n8n -> mcp-n8n -> Sentinel -> Claude Code
```

### Claude Code -> SQLite Query

```
1. Claude Code calls mcp__sentinel-gateway__sqlite_query(sql="SELECT...")
2. Sentinel routes to http://127.0.0.1:3002 (mcp-sqlite HTTP backend)
3. mcp-sqlite uses better-sqlite3 to query /data/*.db
4. Results flow back through Sentinel
```

### Claude Code -> Context7 (Managed stdio)

```
1. Claude Code calls mcp__sentinel-gateway__resolve-library-id(...)
2. Sentinel routes to context7 child process via stdin/stdout
3. Auth, audit, rate limiting all applied by Sentinel
4. Response flows back to Claude Code
```

## Docker Network Topology

```
                    Internet
                       |
                    Caddy (:80, :443)
                       |
            +----------+----------+
            |          |          |
         webproxy   webproxy   webproxy
            |          |          |
       wallybrain   solitaire  authelia    chiasm
       (:8800)      (:8801)    (:9091)
                       |
                 n8n-mcp_default
                       |
            +----------+----------+
            |          |          |
          n8n      mcp-n8n    monitoring
         (:5678)   (:3001)   (9997-9999)
                       |
              sentinel-gateway_sentinelnet
                       |
            +----------+----------+
            |          |          |
      mcp-n8n    mcp-sqlite   sentinel-postgres
      (:3001)    (:3002)      (:5432)
```

**Key:** `mcp-n8n` bridges two networks (`sentinelnet` + `n8n-mcp_default`) because n8n is bound to `127.0.0.1:8567` on the host — unreachable from other containers. The bridge container can reach both the Sentinel network and n8n's network.

## Monitoring Endpoints

Three Node.js HTTP servers run inside the n8n Docker network, consumed by n8n workflows:

| Endpoint | Port | Data | Consumers |
|----------|------|------|-----------|
| `http://cpu-server:9999` | 9999 | `{cpu: float}` | CPU Monitor workflow |
| `http://system-stats:9998` | 9998 | `{memory: {total, used, percent}, disk: {total, used, percent}}` | Memory + Disk Monitor workflows |
| `http://container-health:9997` | 9997 | `{total, running, stopped, unhealthy, containers[]}` | Container Health workflow |

These feed 6 active n8n workflows that send Discord alerts:

| Workflow | Schedule | Trigger |
|----------|----------|---------|
| CPU Monitor | Every 1 min | CPU >= 80% |
| Memory Monitor | Every 5 min | Memory >= 80% |
| Disk Monitor | Every 5 min | Disk >= 80% |
| Container Health | Every 2 min | Any stopped/unhealthy |
| Daily Health Heartbeat | 14:00 UTC | Always (summary embed) |
| Security Reminders | 14:05 UTC | Monday/monthly/quarterly |
