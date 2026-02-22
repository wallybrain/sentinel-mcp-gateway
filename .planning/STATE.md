# Project State: Sentinel Gateway

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-22)

**Core value:** Every MCP tool call passes through one governed point with auth, audit, and rate limiting
**Current focus:** Phase 12 - Network Hardening (next up)

## Current Position

Phase: 12 of 14 (Network Hardening)
Plan: Not started
Status: Phase 11 complete, ready for Phase 12
Last activity: 2026-02-22 -- Phase 11 cutover verified (session 6)

Progress: [########░░] 80%

## Performance Metrics

**v1.0 (completed):** 9 phases, 20 plans, 47 requirements, 138 tests, 3,776 LOC

**v1.1 Velocity:**
- Total plans completed: 4 (10-01, 10-02, 11-01, 11-02)
- Phases completed: 2 (Phase 10, Phase 11)

| Phase | Plan | Duration | Tasks | Files |
|-------|------|----------|-------|-------|
| 10    | 01   | -        | -     | -     |
| 10    | 02   | -        | -     | -     |
| 11    | 01   | 8min     | 3     | 2     |
| 11    | 02   | ~30min   | 4     | 3     |

## Accumulated Context

### Decisions

- [v1.1 Roadmap]: Clean cutover (not parallel) -- 138 tests verify correctness, rollback is trivial
- [v1.1 Roadmap]: Separate Postgres instances -- Sentinel gets its own, not sharing ContextForge's
- [v1.1 Roadmap]: Sidecar migration before cutover -- mcp-n8n/mcp-sqlite must move to Sentinel compose first
- [Phase 10 Research]: Sidecars live in /home/lwb3/mcp-servers/ (NOT ContextForge compose) -- risk is network ownership, not container ownership
- [v1.1 Roadmap]: Phases 10-11-12 strictly sequential; 13 follows 12; 14 gated on 24h stability
- [Phase 11-01]: npm MCP packages globally installed under nvm path (not volatile npx cache)
- [Phase 11-01]: Exa uses .smithery/stdio/index.cjs, Playwright uses @playwright/mcp/cli.js
- [Phase 11-02]: Exa backend disabled (commented out) -- needs EXA_API_KEY, not critical
- [Phase 11-02]: Firecrawl works via env inheritance -- dotenvy loads .env, Command inherits parent env
- [Phase 11-02]: Native binary deployment (not Docker) for Claude Code stdio transport

### Known Gotchas (carried from v1.0)
- Rust builds require `dangerouslyDisableSandbox: true` (bwrap loopback error)
- Docker commands also need sandbox disabled
- npx creates process groups -- must kill entire group, not just parent

### New Gotchas (v1.1)
- MCP servers are project-scoped in `~/.claude.json` under `projects./home/lwb3.mcpServers`
- Reauthenticating Claude Code resets `.claude.json` — ALL MCP server configs lost
- `claude mcp add-json` with python3 `json.dumps()` — `add -e` breaks on base64 `=` chars
- `.env` parsing: `cut -d= -f2` truncates base64 — use `sed 's/^KEY=//'` instead
- Recovery script: `/home/lwb3/sentinel-gateway/add-mcp.sh` re-registers sentinel-gateway
- Stdio child processes inherit parent env (no `env_clear()`) — `.env` vars available to all backends

### Blockers/Concerns

None.

### Pending Todos

None.

## Session Continuity

Last session: 2026-02-22 (session 6)
Stopped at: Phase 11 complete, ready for Phase 12
Resume file: None

### What was accomplished:
- **Phase 11 COMPLETE**: Sentinel is live, all backends verified working
- ContextForge stopped, containers preserved for rollback
- sentinel.toml updated (exa disabled)
- Tool calls confirmed: sqlite_databases, list_workflows, browser tools, context7, sequential-thinking

### Next steps:
1. Phase 12: Network Hardening — bind ports, iptables rules, clean stale networks
2. Phase 13: Monitoring Stack — Prometheus, Grafana, Discord alerts
3. Phase 14: Operations — log rotation, backups, reboot resilience
4. Optional: Remove ContextForge containers when confident in stability

---
*State initialized: 2026-02-22*
*Last updated: 2026-02-22 session 6 — Phase 11 complete*
