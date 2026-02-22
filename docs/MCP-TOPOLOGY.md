# MCP Topology

How Claude Code connects to all MCP servers — governed and ungoverned.

## Connection Overview

Claude Code has two classes of MCP connections:

### 1. Governed (via Sentinel Gateway)

```
Claude Code
    |
    | stdio (JSON-RPC over stdin/stdout)
    v
Docker wrapper container
    |  --network=host
    |  python3 -m mcpgateway.wrapper
    |
    | HTTP POST to http://localhost:9200/servers/{virtual-server-id}/mcp
    | Authorization: Bearer <JWT>
    v
ContextForge Gateway (port 9200)
    |
    | Routes by tool name to registered backends
    |
    +---> mcp-n8n:3000 (Docker mcpnet network)
    |       |
    |       +---> n8n:5678 (Docker n8nnet network)
    |             7 workflows (6 active monitoring + 1 inactive)
    |
    +---> mcp-sqlite:3000 (Docker mcpnet network)
            |
            +---> /home/lwb3/databases/*.db (volume mount)
```

**Auth flow:**
1. Claude Code starts Docker wrapper as stdio subprocess
2. Wrapper reads JWT from env var `MCP_AUTH`
3. Every request includes `Authorization: Bearer <token>` header
4. Gateway validates JWT (HS256, checks exp/iss/aud/jti)
5. Gateway checks RBAC permissions for requested tool
6. Request forwarded to appropriate backend

### 2. Ungoverned (direct stdio)

These MCP servers are launched directly by Claude Code as child processes. No auth, no audit, no rate limiting.

| Server | Launch | Transport | Purpose |
|--------|--------|-----------|---------|
| `context7` | `npx @upstash/context7-mcp` | stdio | Library documentation |
| `firecrawl` | `npx firecrawl-mcp` | stdio | Web scraping |
| `sequential-thinking` | `npx @modelcontextprotocol/server-sequential-thinking` | stdio | Chain-of-thought |
| `exa` | `npx exa-mcp` | stdio | Web search |
| `playwright` | `npx @playwright/mcp` | stdio | Browser automation |

**Risk:** These servers have unrestricted access. A compromised npx package could:
- Exfiltrate data from tool call arguments
- Execute arbitrary code on the host
- Access the filesystem without constraints

## Data Flow Examples

### Governed: Claude Code -> n8n Workflow List

```
1. Claude Code calls mcp__sentinel-gateway__list_workflows()
2. Wrapper serializes JSON-RPC request to stdin
3. Docker wrapper POSTs to http://localhost:9200/servers/7b99a944.../mcp
4. Gateway validates JWT, checks RBAC, logs audit trail
5. Gateway routes to mcp-n8n:3000 (backend registered for this tool)
6. mcp-n8n queries n8n API: GET http://n8n:5678/api/v1/workflows
7. Response flows back: n8n -> mcp-n8n -> gateway -> wrapper -> Claude Code
```

### Governed: Claude Code -> SQLite Query

```
1. Claude Code calls mcp__sentinel-gateway__sqlite_query(sql="SELECT...")
2. Same wrapper/gateway path as above
3. Gateway routes to mcp-sqlite:3000
4. mcp-sqlite uses better-sqlite3 to query /data/music.db
5. Results flow back through the gateway
```

### Ungoverned: Claude Code -> Context7

```
1. Claude Code calls mcp__context7__resolve-library-id(...)
2. Claude Code spawns npx @upstash/context7-mcp as child process
3. Writes JSON-RPC to child's stdin
4. Reads response from child's stdout
5. No auth, no audit, no rate limiting
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
       wallybrain   solitaire  authelia
       (:8800)      (:8801)    (:9091)
                       |
                    n8nnet
                       |
            +----------+----------+
            |          |          |
          n8n      mcp-n8n    monitoring
         (:5678)   (:3000)   (9997-9999)
                       |
                    mcpnet
                       |
            +----------+----------+----------+
            |          |          |          |
         gateway   postgres    redis    mcp-sqlite
         (:4444)   (:5432)    (:6379)   (:3000)
```

**Key:** `mcp-n8n` bridges two networks (`mcpnet` + `n8nnet`) because n8n is bound to `127.0.0.1:8567` on the host — unreachable from other containers. The bridge container can reach both the gateway network and n8n's network.

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

## What the Rust Gateway Should Consolidate

The target is to bring ALL MCP traffic through a single governed chokepoint:

```
BEFORE:                              AFTER:

Claude Code                          Claude Code
    |                                    |
    +---> gateway (governed)             +---> Sentinel Gateway (Rust)
    |       +---> n8n                    |       +---> n8n (HTTP backend)
    |       +---> sqlite                 |       +---> sqlite (HTTP backend)
    |                                    |       +---> context7 (managed stdio)
    +---> context7 (ungoverned)          |       +---> firecrawl (managed stdio)
    +---> firecrawl (ungoverned)         |       +---> exa (managed stdio)
    +---> exa (ungoverned)               |       +---> sequential-thinking (managed)
    +---> sequential-thinking            |       +---> playwright (managed)
    +---> playwright (ungoverned)        |
                                     Single auth, audit, rate limit for everything
```
