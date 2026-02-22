---
phase: 02-mcp-protocol-layer
plan: 02
subsystem: catalog, dispatch, gateway
tags: [mcp, tool-catalog, dispatch-loop, end-to-end, stdio, integration-test]

requires:
  - phase: 02-mcp-protocol-layer/01
    provides: MCP state machine, stdio transport, handle_initialize
provides:
  - ToolCatalog aggregating tools from multiple backends with collision resolution
  - Central dispatch loop (run_dispatch) routing JSON-RPC through state machine
  - Working MCP binary handling initialize/initialized/tools-list/ping over stdio
affects: [http-backend-routing, tool-call-dispatch, rbac-filtering]

tech-stack:
  added: []
  patterns: [catalog aggregation with collision prefixing, channel-based dispatch loop, lenient config loading for phased development]

key-files:
  created:
    - src/catalog/mod.rs
    - src/gateway.rs
    - tests/catalog_test.rs
    - tests/gateway_integration_test.rs
  modified:
    - src/lib.rs
    - src/main.rs
    - src/config/mod.rs

key-decisions:
  - "Lenient config loading (load_config_lenient) skips auth/postgres validation for early phases"
  - "Tool collision resolution prefixes with backend_name__tool_name"
  - "Box::leak for catalog in integration tests to satisfy 'static lifetime"

patterns-established:
  - "Dispatch loop as async function consuming channels (not trait-based)"
  - "Stub catalog factory function for testing and development"
  - "Lenient vs strict config validation split for phased development"

requirements-completed: [PROTO-02, PROTO-03, PROTO-06]

duration: 4min
completed: 2026-02-22
---

# Phase 2 Plan 2: Tool Catalog & Dispatch Loop Summary

**ToolCatalog aggregating stub backends with collision prefixing, channel-based dispatch loop handling full MCP lifecycle, wired into main.rs as working stdio MCP server**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-22T02:45:15Z
- **Completed:** 2026-02-22T02:49:00Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Created ToolCatalog struct aggregating tools from multiple backends with name collision prefixing
- Built central dispatch loop (run_dispatch) routing messages through MCP state machine
- Wired main.rs with stdio transport, dispatch loop, and stub catalog as working MCP binary
- Added load_config_lenient() to skip auth/postgres secrets during early phases
- 11 new tests (5 catalog + 6 gateway integration), 41 total passing
- Binary responds to initialize/tools-list/ping over stdio

## Task Commits

1. **Task 1: Tool catalog and dispatch loop** - `7eeb364` (feat)
2. **Task 2: Catalog unit tests and end-to-end integration test** - `828ceeb` (test)

## Files Created/Modified
- `src/catalog/mod.rs` - ToolCatalog struct, register_backend, all_tools, route, collision handling, create_stub_catalog
- `src/gateway.rs` - run_dispatch() central dispatch loop with state gating
- `src/lib.rs` - Added catalog and gateway module exports
- `src/main.rs` - Wired transport + dispatch + catalog, uses lenient config loading
- `src/config/mod.rs` - Added load_config_lenient() and validate_backends()
- `tests/catalog_test.rs` - 5 tests for catalog registration, routing, collisions
- `tests/gateway_integration_test.rs` - 6 end-to-end tests through dispatch loop

## Decisions Made
- Added `load_config_lenient()` that skips JWT secret and Postgres URL resolution -- the config file requires `[auth]` and `[postgres]` sections but the env vars don't need to exist. This avoids requiring dummy secrets during Phase 2 while keeping the full `validate()` intact for later phases.
- Tool name collisions resolved by prefixing the second registration with `{backend_name}__{tool_name}`. First registration keeps the bare name.
- Integration tests use `Box::leak` to give the catalog a `'static` lifetime for the spawned dispatch task. Acceptable in test code.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing functionality] Config validation fails without env vars**
- **Found during:** Task 1
- **Issue:** `load_config()` calls `resolve_jwt_secret()` and `resolve_url()` which fail without JWT_SECRET_KEY and DATABASE_URL env vars
- **Fix:** Added `load_config_lenient()` and `validate_backends()` to separate backend validation from secret resolution
- **Files modified:** src/config/mod.rs, src/main.rs

**2. [Rule 1 - Bug] Clippy warning for missing Default impl**
- **Found during:** Task 1
- **Issue:** ToolCatalog has `new()` but no `Default` impl
- **Fix:** Added `impl Default for ToolCatalog`
- **Files modified:** src/catalog/mod.rs

## Issues Encountered
None beyond the deviations above.

## User Setup Required
None - binary works with `cargo run -- --config sentinel.toml` without any env vars.

## Next Phase Readiness
- Working MCP server binary accepting stdio connections
- Tool catalog ready for real backend registration (Phase 3)
- Dispatch loop ready for tools/call routing (Phase 3)
- 41 tests passing, zero clippy warnings, clean release build

---
*Phase: 02-mcp-protocol-layer*
*Completed: 2026-02-22*
