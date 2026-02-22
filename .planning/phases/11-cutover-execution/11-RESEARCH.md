# Phase 11 Research: Cutover Execution

## Current State

### ContextForge (the predecessor)
- **Container**: `mcp-context-forge-gateway-1` running on `127.0.0.1:9200→4444`
- **Transport**: HTTP (streamable-http) — Claude Code uses a Python wrapper (`mcpgateway.wrapper`) for stdio→HTTP bridging
- **Registration**: In `~/.claude/settings.json` as `sentinel-gateway`, command runs `python3 -m mcpgateway.wrapper` with `MCP_SERVER_URL` and `MCP_AUTH` env vars
- **Token**: JWT in `/tmp/claude-1001/gateway-token.txt`, iss=`mcpgateway`, aud=`mcpgateway-api`
- **Tools**: 19 tools (9 n8n + 10 sqlite) via HTTP sidecars

### Sentinel (the replacement)
- **4 containers running**: gateway (9202→9200), postgres, mcp-n8n, mcp-sqlite — all healthy
- **Transport**: Native stdio (reads JSON-RPC from stdin, writes to stdout) — no wrapper needed
- **Auth**: Session-level JWT via SENTINEL_TOKEN env var (validated at startup)
- **Docker config**: `sentinel-docker.toml` (HTTP-only backends, 0.0.0.0 binding)
- **Native config**: `sentinel.toml` (all 7 backends including 5 stdio)

### Ungoverned MCP Servers
5 servers run directly in Claude Code (not through any gateway):
- context7, firecrawl, exa, sequential-thinking, playwright
- These are unaffected by the cutover

## Key Architectural Decision: Native Binary vs Docker

### Option A: Native Binary (RECOMMENDED)
Claude Code spawns `sentinel-gateway --config sentinel.toml` directly. This is what the binary was designed for:
- stdio transport built-in (src/transport/stdio.rs)
- Can spawn stdio child processes (context7, firecrawl, etc.) since Node.js is on the host
- No Docker networking complexity
- Session-level JWT auth works naturally (env var passed at spawn)
- Postgres can still run in Docker (gateway connects via localhost)

**Pros**: Simplest, fastest, all 7 backends work, no bridge needed
**Cons**: Must build the binary on VPS, binary not containerized

### Option B: Docker + Bridge
Keep gateway in Docker, write a stdio-to-HTTP bridge (like ContextForge's wrapper).

**Pros**: Containerized
**Cons**: Need to write/maintain a bridge, HTTP-only backends, more moving parts

### Decision: Option A — Native Binary
Sentinel was designed as a native stdio MCP server. The Docker deployment is useful for the sidecar containers (mcp-n8n, mcp-sqlite) and postgres, but the gateway binary should run natively so Claude Code can spawn it directly.

## Cutover Sequence

### Phase 11 will:
1. Build Sentinel release binary on VPS
2. Stop the Docker gateway container (keep sidecars + postgres running)
3. Generate a new JWT token for Sentinel
4. Update `~/.claude/settings.json` to spawn the native Sentinel binary
5. Update `sentinel.toml` to connect to Docker sidecars via localhost ports
6. Verify all tools work end-to-end
7. Document rollback procedure

### Port Changes
- Docker sidecars need host port exposure:
  - mcp-n8n: expose `127.0.0.1:3001:3000` (new)
  - mcp-sqlite: expose `127.0.0.1:3002:3000` (new)
- sentinel.toml backends change from Docker hostnames to localhost:
  - `http://mcp-n8n:3000` → `http://127.0.0.1:3001`
  - `http://mcp-sqlite:3000` → `http://127.0.0.1:3002`
- Postgres: already on sentinelnet, but native binary needs `127.0.0.1:5432`
  - Expose `127.0.0.1:5432:5432` on postgres container
- Gateway Docker container: stop (or remove gateway service from compose entirely)
- Port 9200: no longer needed (native binary uses stdio, not HTTP)
- Port 9201: health server still runs (native binary binds 127.0.0.1:9201)

### Claude Code Registration
```json
{
  "sentinel-gateway": {
    "command": "/home/lwb3/sentinel-gateway/target/release/sentinel-gateway",
    "args": ["--config", "/home/lwb3/sentinel-gateway/sentinel.toml"],
    "env": {
      "JWT_SECRET_KEY": "...",
      "SENTINEL_TOKEN": "...",
      "DATABASE_URL": "postgres://sentinel:PASSWORD@127.0.0.1:5432/sentinel"
    }
  }
}
```

### Rollback
If cutover fails:
1. Revert `~/.claude/settings.json` to ContextForge wrapper command
2. Start Docker gateway container: `docker compose up -d gateway`
3. Restart ContextForge: `docker compose -f /home/lwb3/mcp-context-forge/docker-compose.yml up -d`

## CUT-04: "All 7 backends" Clarification
The requirement says verify all 7 backends. In the native deployment:
- HTTP: n8n (via localhost:3001), sqlite (via localhost:3002)
- Stdio: context7, firecrawl, exa, sequential-thinking, playwright (spawned by Sentinel as child processes)

All 7 backends can work with the native binary since Node.js is available on the VPS. The Docker-only deployment was limited to 2 HTTP backends.

## Token Management
- New JWT signed with Sentinel's JWT_SECRET_KEY (raw string, HS256)
- Claims: sub, role=admin, iss=sentinel-gateway, aud=sentinel-api, exp (1 year)
- Passed as SENTINEL_TOKEN env var in Claude Code MCP config
- No need for token file — env var is the mechanism
