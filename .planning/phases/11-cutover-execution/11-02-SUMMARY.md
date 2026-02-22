# Phase 11 Plan 02 Summary: Cutover Execution

**Completed:** 2026-02-22 (sessions 4-6)

## What Was Done

### Task 1: Stop ContextForge, release port 9200
- Stopped all ContextForge containers (gateway, postgres, redis, pgbouncer, fast_time_server)
- Containers preserved (not removed) for rollback
- Bound all ContextForge services to 127.0.0.1 before stopping
- Port 9200 released for Sentinel

### Task 2: Register sentinel-gateway in Claude Code MCP config
- Created `add-mcp.sh` script using `claude mcp add-json` + python3 for safe JSON handling
- Registered sentinel-gateway as project-scoped MCP server in `.claude.json`
- Discovered: `claude mcp add -e` breaks on base64 values with `=` padding
- Discovered: reauthenticating Claude Code wipes all MCP configs from `.claude.json`

### Task 3: Verify health, backends, tool discovery
- Confirmed after Claude Code restart (session 6)
- sqlite backend: `sqlite_databases` returned databases
- n8n backend: `list_workflows` returned all 7 workflows
- Browser/playwright tools: working
- context7: working (resolve-library-id, query-docs)
- sequential-thinking: working
- Firecrawl: working (env inherited via dotenvy)

### Task 4: Final user approval
- Cutover confirmed working by user in session 6
- STATE.md updated to Phase 12
- ROADMAP.md updated to mark Phase 11 complete

## Config Changes

- `sentinel.toml`: Exa backend commented out (needs EXA_API_KEY, not critical)
- `add-mcp.sh`: New script for re-registering sentinel-gateway MCP entry

## Key Learnings

- Rust's `Command` inherits parent env by default — no need for per-backend `env` config in TOML for vars loaded via `dotenvy`
- Claude Code MCP config lives in `~/.claude.json` under `projects.<path>.mcpServers`, NOT in `~/.claude/settings.json`
- `claude mcp add-json` is the reliable registration method (handles special chars in env values)

## Rollback Procedure (documented, not tested)

```bash
docker compose -f /home/lwb3/mcp-context-forge/docker-compose.yml start
claude mcp remove sentinel-gateway
# Restart Claude Code
```
