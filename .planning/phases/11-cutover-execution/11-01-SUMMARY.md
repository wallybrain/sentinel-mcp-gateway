---
phase: 11-cutover-execution
plan: 01
subsystem: infra
tags: [rust, docker, native-binary, jwt, mcp, stdio, postgres]

requires:
  - phase: 10-pre-cutover-preparation
    provides: "Docker sidecars, sentinel.toml, Cargo.toml all configured and building"
provides:
  - "Release binary at target/release/sentinel-gateway"
  - "docker-compose.yml with 3 services (no gateway), localhost port exposure"
  - "sentinel.toml with localhost HTTP backend URLs and correct nvm stdio paths"
  - "JWT token for native deployment at /tmp/claude-1001/sentinel-token.txt"
affects: [11-02, 12-cutover-switchover]

tech-stack:
  added: [exa-mcp-server, firecrawl-mcp, context7-mcp, sequential-thinking, playwright-mcp]
  patterns: [native-binary-with-docker-sidecars, localhost-port-mapping]

key-files:
  created: []
  modified:
    - docker-compose.yml
    - sentinel.toml

key-decisions:
  - "Globally installed npm MCP packages under nvm path instead of using volatile npx cache"
  - "Exa MCP uses .smithery/stdio/index.cjs entry point (smithery build system)"
  - "Playwright MCP uses @playwright/mcp/cli.js (not @anthropic/mcp-playwright)"

patterns-established:
  - "Native binary + Docker sidecars: binary runs on host, sidecars expose 127.0.0.1 ports"
  - "Stdio backend paths: /home/lwb3/.nvm/versions/node/v20.20.0/lib/node_modules/"

requirements-completed: [CUT-02, CUT-03]

duration: 8min
completed: 2026-02-22
---

# Phase 11 Plan 01: Cutover Preparation Summary

**Release binary built, Docker sidecars reconfigured with localhost ports, sentinel.toml updated to 127.0.0.1 backends with correct nvm-based stdio paths**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-22T17:28:53Z
- **Completed:** 2026-02-22T17:36:36Z
- **Tasks:** 3
- **Files modified:** 2

## Accomplishments
- Built optimized release binary (cargo build --release) verified with --help
- Removed gateway Docker service, added localhost port mappings for postgres (5432), mcp-n8n (3001), mcp-sqlite (3002)
- Updated sentinel.toml HTTP backend URLs from Docker hostnames to 127.0.0.1 and fixed all 5 stdio backend paths from /usr/local/lib to nvm global node_modules
- Generated 1-year JWT token for native deployment
- Restarted all 3 Docker services, verified healthy and reachable from localhost

## Task Commits

Each task was committed atomically:

1. **Task 1: Build Sentinel release binary** - no commit (binary in gitignored target/)
2. **Task 2: Update docker-compose.yml and sentinel.toml, generate JWT token** - `f40b81d` (feat)
3. **Task 3: Restart Docker sidecars and verify localhost connectivity** - no commit (runtime-only, no file changes)

## Files Created/Modified
- `docker-compose.yml` - Removed gateway service, added port mappings for 3 remaining services
- `sentinel.toml` - HTTP backends to localhost:3001/3002, stdio paths to nvm global modules

## Decisions Made
- Globally installed MCP npm packages instead of relying on npx cache paths (volatile hash-based directories)
- Used exa-mcp-server's `.smithery/stdio/index.cjs` since the package requires smithery build tooling not available locally
- Used `@playwright/mcp/cli.js` as entry point (the old `@anthropic/mcp-playwright` package name doesn't exist on npm)
- Removed orphaned sentinel-gateway Docker container left from previous compose config

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed stdio backend paths from /usr/local/lib to nvm global path**
- **Found during:** Task 2 (sentinel.toml update)
- **Issue:** All 5 stdio backend paths pointed to /usr/local/lib/node_modules which doesn't exist -- npm global installs go to nvm path
- **Fix:** Globally installed packages and updated all paths to /home/lwb3/.nvm/versions/node/v20.20.0/lib/node_modules/
- **Files modified:** sentinel.toml
- **Verification:** All 5 paths verified to exist on filesystem
- **Committed in:** f40b81d

**2. [Rule 3 - Blocking] Fixed exa-mcp-server entry point path**
- **Found during:** Task 2 (sentinel.toml update)
- **Issue:** Plan referenced `exa-mcp-server/build/index.js` but exa uses smithery build system producing `.smithery/stdio/index.cjs`
- **Fix:** Updated path to .smithery/stdio/index.cjs
- **Files modified:** sentinel.toml
- **Verification:** File exists at correct path
- **Committed in:** f40b81d

**3. [Rule 3 - Blocking] Fixed playwright MCP package name**
- **Found during:** Task 2 (sentinel.toml update)
- **Issue:** Plan referenced `@anthropic/mcp-playwright` which doesn't exist on npm -- correct package is `@playwright/mcp`
- **Fix:** Installed @playwright/mcp globally, updated sentinel.toml to use cli.js entry point
- **Files modified:** sentinel.toml
- **Verification:** Package installed and path verified
- **Committed in:** f40b81d

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All fixes necessary for stdio backends to function. No scope creep.

## Issues Encountered
- Orphaned sentinel-gateway container from previous compose config -- removed with `docker rm -f`
- sentinelnet network showed "Resource is still in use" during `docker compose down` -- resolved by removing orphan container first

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Binary ready to run natively on host
- All sidecars healthy and reachable via localhost
- JWT token available for authentication
- Ready for Plan 02: Claude Code MCP integration and end-to-end verification

## Self-Check: PASSED

- FOUND: 11-01-SUMMARY.md
- FOUND: commit f40b81d
- FOUND: release binary
- FOUND: JWT token

---
*Phase: 11-cutover-execution*
*Completed: 2026-02-22*
