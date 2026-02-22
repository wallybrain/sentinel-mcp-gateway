# Sentinel Gateway

## Overview

Rust-based enterprise MCP gateway — single binary replacing IBM ContextForge (Python/FastAPI).
**Status: v1.0 shipped, v1.1 in progress (cutover complete, hardening phases remain).**

## Key Paths

| Path | Purpose |
|------|---------|
| `src/` | Rust source (3,776 LOC, 138 tests) |
| `sentinel.toml` | Native runtime config (7 backends) |
| `docker-compose.yml` | Sidecar services (postgres, mcp-n8n, mcp-sqlite) |
| `.env` | Secrets — JWT, Postgres, Sentinel token, API keys (gitignored) |
| `.planning/` | GSD project planning (roadmap, phases, state) |
| `docs/` | Architecture and requirements documentation |
| `add-mcp.sh` | Re-register sentinel-gateway in Claude Code MCP config |

## Build & Run

```bash
# Build (needs sandbox disabled — bwrap loopback error)
cargo build --release

# Binary at target/release/sentinel-gateway (~14 MB)
# Spawned automatically by Claude Code as MCP server (stdio transport)

# Sidecars
docker compose up -d  # postgres, mcp-n8n, mcp-sqlite
```

## Architecture

- **Transport**: stdio (Claude Code spawns binary directly)
- **Auth**: Session-level JWT (validated once at startup)
- **Backends**: 2 HTTP (mcp-n8n:3001, mcp-sqlite:3002) + 5 stdio (context7, firecrawl, playwright, sequential-thinking, ollama)
- **Exa**: Disabled in sentinel.toml (needs EXA_API_KEY)
- **Env inheritance**: `dotenvy` loads `.env` → child stdio processes inherit all vars

## Gotchas

- Rust/Docker builds need `dangerouslyDisableSandbox: true`
- `claude mcp add-json` for registration (not `add -e`, breaks on base64 `=`)
- Reauthenticating Claude Code wipes `.claude.json` MCP configs — run `add-mcp.sh` to restore
- JWT: Rust uses `secret.as_bytes()` (raw string), NOT base64-decoded
- `sentinel.toml` listen must be `0.0.0.0` inside Docker (not 127.0.0.1)
