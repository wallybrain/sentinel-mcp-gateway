---
phase: 15-cutover-gap-closure
plan: 01
subsystem: infra
tags: [mcp, config, env-wiring, cleanup]

requires:
  - phase: 11-cutover
    provides: Native binary deployment, add-mcp.sh registration script
  - phase: 12-network-hardening
    provides: iptables rules targeting port 9201
provides:
  - Durable FIRECRAWL_API_KEY wiring in add-mcp.sh (survives Claude Code reinstall)
  - Explicit health_listen in sentinel.toml (no silent default mismatch)
  - Clean config state (no orphaned files or duplicate MCP registrations)
  - Accurate REQUIREMENTS.md tracking for CUT-01 and CUT-04
affects: [15-02-rollback-test, 13-monitoring]

tech-stack:
  added: []
  patterns: [sed-based .env extraction for MCP env dict]

key-files:
  created: []
  modified:
    - add-mcp.sh
    - sentinel.toml
    - CLAUDE.md
    - .planning/REQUIREMENTS.md

key-decisions:
  - "FIRECRAWL_API_KEY wired via sed extraction (same pattern as other vars) rather than relying on env inheritance"
  - "health_listen set to 127.0.0.1:9201 matching existing iptables rules"

patterns-established:
  - "All env vars used by MCP registration must be explicitly extracted in add-mcp.sh, not inherited"

requirements-completed: [CUT-01, CUT-04]

duration: 2min
completed: 2026-02-22
---

# Phase 15 Plan 01: Config Hardening and Gap Closure Summary

**Durable FIRECRAWL_API_KEY env wiring, explicit health_listen config, orphaned file cleanup, and REQUIREMENTS.md sync**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-22T22:33:22Z
- **Completed:** 2026-02-22T22:34:48Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- add-mcp.sh now explicitly wires FIRECRAWL_API_KEY (survives Claude Code reinstall without relying on env inheritance)
- sentinel.toml has explicit health_listen = "127.0.0.1:9201" (matches iptables, no silent default drift)
- Removed orphaned sentinel-docker.toml and duplicate MCP registration
- REQUIREMENTS.md accurately reflects CUT-01 and CUT-04 as complete

## Task Commits

Each task was committed atomically:

1. **Task 1: Harden env wiring and config explicitness** - `4c59e7c` (fix)
2. **Task 2: Clean up orphaned files and duplicate registrations** - `abd2bff` (chore)
3. **Task 3: Update REQUIREMENTS.md checkboxes and text** - `9352036` (docs)

## Files Created/Modified
- `add-mcp.sh` - Added FIRECRAWL_API_KEY extraction and env dict entry
- `sentinel.toml` - Added explicit health_listen = "127.0.0.1:9201"
- `sentinel-docker.toml` - Deleted (orphaned Docker-era config)
- `CLAUDE.md` - Removed sentinel-docker.toml from key paths table
- `.planning/REQUIREMENTS.md` - Marked CUT-01 and CUT-04 complete, updated traceability

## Decisions Made
- FIRECRAWL_API_KEY uses the same sed extraction pattern as other vars for consistency
- health_listen value 127.0.0.1:9201 chosen to match existing iptables rules and fix-iptables.sh

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Plan 02 (rollback test) can proceed -- CUT-05 remains unchecked pending that execution
- Phase 13 (monitoring) unblocked by explicit health_listen config

## Self-Check: PASSED

- FOUND: add-mcp.sh
- FOUND: sentinel.toml
- FOUND: .planning/REQUIREMENTS.md
- FOUND: 15-01-SUMMARY.md
- OK: sentinel-docker.toml deleted
- FOUND: commit 4c59e7c
- FOUND: commit abd2bff
- FOUND: commit 9352036

---
*Phase: 15-cutover-gap-closure*
*Completed: 2026-02-22*
