---
phase: 08-stdio-backend-management
plan: 03
subsystem: gateway
tags: [stdio, dispatch, backend-enum, integration, startup]
dependency_graph:
  requires: [StdioBackend, run_supervisor, discover_stdio_tools]
  provides: [Backend, unified-dispatch, stdio-startup-sequence]
  affects: [gateway, main, backend]
tech_stack:
  added: [tempfile-3]
  patterns: [backend-enum-dispatch, supervisor-startup-sequence, mock-mcp-server-tests]
key_files:
  created:
    - tests/stdio_integration.rs
  modified:
    - src/backend/mod.rs
    - src/gateway.rs
    - src/main.rs
    - tests/gateway_integration_test.rs
    - tests/health_integration_test.rs
    - Cargo.toml
decisions:
  - Backend enum with Http/Stdio variants for unified send() dispatch
  - CancellationToken created before stdio spawning (needed by supervisors)
  - 30s timeout for initial stdio backend tool discovery
  - 5s timeout for supervisor shutdown during ordered shutdown sequence
  - Python mock MCP server for integration tests (portable, readable)
metrics:
  duration: 8min
  completed: 2026-02-22T07:02Z
---

# Phase 8 Plan 3: Gateway Integration with Backend Enum Summary

Backend enum unifies HTTP and stdio dispatch; main.rs spawns stdio supervisors from config, registers tools in catalog, terminates process groups on shutdown; 3 integration tests prove end-to-end stdio routing through the full dispatch loop

## What Was Built

### Backend enum (src/backend/mod.rs)
- `Backend::Http(HttpBackend)` and `Backend::Stdio(StdioBackend)` variants
- `send()` method delegates to the appropriate transport
- Derives `Clone` for use in backends map

### Gateway dispatch refactor (src/gateway.rs)
- `run_dispatch()` and `handle_tools_call()` accept `HashMap<String, Backend>` instead of `HashMap<String, HttpBackend>`
- All enforcement (kill switch, rate limiting, RBAC, circuit breaker, audit, ID remapping) works identically for both backend types
- Zero behavior change for HTTP backends -- pure type refactor

### Stdio backend startup sequence (src/main.rs)
- CancellationToken created early (before stdio spawning) for supervisor access
- Filters stdio backends from config, spawns supervisor tasks via `run_supervisor()`
- Waits up to 30s for each backend's MCP handshake to complete
- Registers discovered tools in catalog, inserts `Backend::Stdio` into backends_map
- Initializes health map entries and circuit breakers for stdio backends
- On shutdown: cancels supervisors (process group kill), waits up to 5s per supervisor handle

### Integration tests (tests/stdio_integration.rs)
- Python mock MCP server script: handles initialize, tools/list, tools/call
- `test_stdio_tool_discovery_returns_tools`: spawn + discover_stdio_tools returns 1 tool
- `test_stdio_tools_call_through_dispatch`: full MCP handshake + tools/list + tools/call through dispatch loop
- `test_kill_process_group_terminates_stdio_child`: verify process group kill terminates child

### Existing test updates
- `tests/gateway_integration_test.rs`: `HashMap<String, HttpBackend>` -> `HashMap<String, Backend>`
- `tests/health_integration_test.rs`: same type update

## Task Completion

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Create Backend enum and update gateway dispatch | b34db9e | src/backend/mod.rs, src/gateway.rs, src/main.rs, tests/gateway_integration_test.rs, tests/health_integration_test.rs |
| 2 | Wire stdio backend spawning and integration tests | f514cc9 | src/main.rs, tests/stdio_integration.rs, Cargo.toml, Cargo.lock |

## Verification

- `cargo check` -- clean compile, no warnings
- `cargo test` -- 125/125 tests passing (122 existing + 3 new)
  - test_stdio_tool_discovery_returns_tools: spawns mock MCP server, discovers 1 tool
  - test_stdio_tools_call_through_dispatch: full dispatch loop routes tools/call to stdio backend
  - test_kill_process_group_terminates_stdio_child: verifies process group kill
- `cargo build --release` -- single binary produced successfully

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed RBAC config in integration test**
- **Found during:** Task 2 test execution
- **Issue:** Default RbacConfig has no roles, so admin caller had no permissions and tools/list returned empty
- **Fix:** Added admin role with wildcard permissions to test RBAC config (matching pattern from existing tests)
- **Files modified:** tests/stdio_integration.rs

**2. [Rule 3 - Blocking] Updated existing integration tests for Backend type**
- **Found during:** Task 1 test compilation
- **Issue:** gateway_integration_test.rs and health_integration_test.rs used `HashMap<String, HttpBackend>` which no longer matches run_dispatch signature
- **Fix:** Changed to `HashMap<String, Backend>` and updated imports
- **Files modified:** tests/gateway_integration_test.rs, tests/health_integration_test.rs

## Decisions Made

1. **Backend enum with Clone derive** -- both HttpBackend and StdioBackend are Clone, so Backend can be too
2. **CancellationToken created early** -- moved before stdio spawning so supervisors can receive it
3. **30s timeout for tool discovery** -- generous for slow startup (npm install, etc); supervisor retries in background on timeout
4. **5s shutdown timeout per supervisor** -- enough for SIGTERM + process exit; logs warning if exceeded
5. **Python mock MCP server** -- python3 is available on the system, script is readable and portable for testing

## Self-Check: PASSED

- All key files exist (src/backend/mod.rs, src/gateway.rs, src/main.rs, tests/stdio_integration.rs)
- Both commits verified (b34db9e, f514cc9)
- 125/125 tests passing
- Release binary builds successfully
