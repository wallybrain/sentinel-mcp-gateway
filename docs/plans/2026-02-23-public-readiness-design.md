# Public-Readiness Design — Sentinel Gateway

**Date:** 2026-02-23
**Status:** Approved
**License:** BSL 1.1 (Change License: Apache 2.0, Change Date: 2030-02-23)

## Goal

Make the existing sentinel-gateway repository clone-and-run ready for any developer on a naked VPS, while maintaining it as a single private repo with no maintenance overhead. Position for OpenClaw security gap.

## Approach

In-place cleanup (Approach A). One repo, parameterize paths, add docs, add license. No separate fork, no clean branch.

## File Changes

### CREATE

| File | Purpose |
|------|---------|
| `LICENSE` | BSL 1.1 full text |
| `sentinel.toml.example` | Generic config with `npx` commands, all backends commented out as examples |
| `sentinel-docker.toml` | Docker-specific config (HTTP backends only) |
| `docker-compose.override.yml.example` | Template for server-specific path overrides |
| `scripts/setup.sh` | Interactive setup — generates `.env`, detects Node paths, creates `sentinel.toml` |
| `docs/DEPLOYMENT.md` | Step-by-step naked VPS guide |
| `docs/OPENCLAW.md` | OpenClaw integration guide — why, how, architecture |

### MODIFY

| File | Change |
|------|--------|
| `README.md` | Rewrite for public audience — quick-start, OpenClaw callout, license badge, links |
| `.gitignore` | Add `sentinel.toml`, `docker-compose.override.yml`, `.planning/` |
| `docker-compose.yml` | Relative paths with env var fallbacks, override pattern |
| `add-mcp.sh` | Auto-detect `$PWD`, `which node`, portable |
| `Cargo.toml` | `license = "BSL-1.1"` |
| `.env.example` | Add `FIRECRAWL_API_KEY`, mark required vs optional vars |

### REMOVE FROM TRACKING

| Path | Reason |
|------|--------|
| `.planning/**` | Internal GSD project management |

## Configuration Strategy

### sentinel.toml.example
- All backends commented out as examples
- HTTP backends show URL pattern
- stdio backends use `npx -y <package>` (portable, no absolute paths)
- Inline comments explain each field

### sentinel-docker.toml
- HTTP backends only (n8n, sqlite)
- No stdio backends (can't run inside minimal Docker container)
- Copied into container by Dockerfile

### docker-compose.yml
- Build contexts use env var with relative fallback: `${N8N_MCP_PATH:-./sidecars/n8n-mcp-server}`
- Volume mounts use env var: `${SQLITE_DB_DIR:-./data}:/data`
- User creates `docker-compose.override.yml` for server-specific paths

## OpenClaw Integration

### Positioning
Sentinel Gateway fills the security gap between OpenClaw and MCP servers. OpenClaw uses the same `mcpServers` JSON format as Claude — Sentinel slots in as a transparent proxy.

### Architecture (for docs)
```
OpenClaw agent
    |  (unprotected MCP calls)
    v
Sentinel Gateway
    |  JWT auth, RBAC, rate limiting, audit, circuit breakers
    |
    +---> MCP Server A (stdio or HTTP)
    +---> MCP Server B
    +---> MCP Server N
```

### docs/OPENCLAW.md contents
1. Why OpenClaw needs a gateway (CVEs, exposed instances, Pynt research)
2. How Sentinel fills the gap (feature mapping to OpenClaw security gaps)
3. Step-by-step: point OpenClaw at Sentinel
4. Example `mcpServers` config for OpenClaw
5. Recommended RBAC policies for OpenClaw agents

## License (BSL 1.1)

- **Licensor:** Wally Blanchard
- **Licensed Work:** Sentinel Gateway (all versions)
- **Additional Use Grant:** Production use permitted, except offering the Licensed Work as a managed/hosted MCP gateway service to third parties
- **Change License:** Apache License 2.0
- **Change Date:** Four years from each release date
- **Notice:** Standard BSL 1.1 notice

## Going-Public Checklist (for later)

Include in DEPLOYMENT.md as a reference section:
1. Run `git filter-repo` to strip `.planning/` from all commits
2. Strip `/home/lwb3/` references from commit history
3. Remove `CLAUDE.md` from history (internal project notes)
4. Flip GitHub visibility to public
5. Add GitHub Actions CI (cargo test, cargo clippy)
6. Add shields.io badges (build, license, version)

## Implementation Order

1. Add BSL 1.1 LICENSE file
2. Update .gitignore (sentinel.toml, override, .planning/)
3. Remove .planning/ from git tracking
4. Create sentinel.toml.example
5. Create sentinel-docker.toml
6. Create docker-compose.override.yml.example
7. Update docker-compose.yml (parameterize paths)
8. Rewrite add-mcp.sh (portable)
9. Update .env.example (required vs optional)
10. Update Cargo.toml license
11. Write docs/DEPLOYMENT.md
12. Write docs/OPENCLAW.md
13. Rewrite README.md
14. Commit and push
