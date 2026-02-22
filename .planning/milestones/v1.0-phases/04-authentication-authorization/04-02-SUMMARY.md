---
phase: 04-authentication-authorization
plan: 02
subsystem: gateway
tags: [auth, rbac, dispatch, integration, jwt]
dependency_graph:
  requires: [src/auth/jwt.rs (CallerIdentity), src/auth/rbac.rs (is_tool_allowed, Permission), config/types.rs (RbacConfig)]
  provides: [Auth-gated dispatch loop, JWT validation at session start]
  affects: [All downstream phases (dispatch is now auth-aware)]
tech_stack:
  added: []
  patterns: [caller identity threading, RBAC dispatch filtering, dev-mode auth bypass]
key_files:
  created: []
  modified:
    - src/gateway.rs
    - src/main.rs
    - tests/gateway_integration_test.rs
decisions:
  - CallerIdentity passed directly to run_dispatch (not JwtValidator) for testability and separation of concerns
  - JWT validation happens in main.rs before dispatch, gateway.rs only handles RBAC
  - Default admin identity when no caller provided (dev/test mode)
  - AUTHZ_ERROR is -32003 (distinct from -32001 auth error and -32002 not-initialized)
metrics:
  duration: 3m 35s
  completed: 2026-02-22T04:14:39Z
  tasks: 2/2
  files: 3
  tests_added: 9
  total_tests: 85
---

# Phase 4 Plan 2: Auth Integration into Dispatch Loop Summary

CallerIdentity + RBAC threading into gateway dispatch -- tools/list filtered by role, tools/call blocked with -32003 for unauthorized access.

## Tasks Completed

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | Wire auth into dispatch loop and main.rs | 3cee323 | src/gateway.rs, src/main.rs |
| 2 | Update existing tests and add auth integration tests | 89d141a | tests/gateway_integration_test.rs |

## What Was Built

### Gateway Auth Integration (`src/gateway.rs`)
- `run_dispatch()` now accepts `caller: Option<CallerIdentity>` and `rbac_config: &RbacConfig`
- Default admin identity when caller is None (dev/test mode)
- `tools/list` filters via `is_tool_allowed(role, tool, Permission::Read, rbac)`
- `tools/call` checks `is_tool_allowed(role, tool, Permission::Execute, rbac)` before routing
- Unauthorized tools/call returns JSON-RPC error -32003 ("Permission denied for tool: {name}")
- `ping`, `initialize`, `notifications/initialized` exempt from tool-level RBAC

### Session Authentication (`src/main.rs`)
- JWT secret read from config-specified env var at startup
- If secret empty/missing: auth disabled (dev mode warning logged)
- If secret present: reads `SENTINEL_TOKEN` env var, validates via `JwtValidator`
- Authenticated identity (subject, role) logged and passed to dispatch
- Invalid/missing token causes immediate exit with error

### Integration Tests (9 new, 85 total)
- `test_viewer_sees_all_tools_in_list`: viewer with tools.read sees all 4 tools
- `test_viewer_cannot_call_tools`: viewer tools/call returns -32003
- `test_developer_denied_tool_hidden_in_list`: write_query hidden from developer
- `test_developer_denied_tool_blocked_in_call`: write_query call returns -32003
- `test_developer_can_call_allowed_tool`: read_query passes RBAC (hits backend error, not -32003)
- `test_unknown_role_sees_no_tools`: unrecognized role gets empty catalog
- `test_unknown_role_cannot_call`: unrecognized role gets -32003
- `test_admin_wildcard_sees_all_tools`: admin with ["*"] sees all 4 tools
- `test_admin_denied_tool_override`: admin with denied_tools still blocked

## Deviations from Plan

### Design Refinement (Plan-prescribed)

**1. CallerIdentity instead of JwtValidator in run_dispatch**
- **Found during:** Task 1 (plan itself recommended this in Task 2)
- **Issue:** Plan initially had JwtValidator in gateway.rs, then revised to CallerIdentity for better separation
- **Fix:** Implemented the revised approach from the start -- JWT validation in main.rs, CallerIdentity passed to gateway.rs
- **Rationale:** Cleaner testability (no env var mocking), better separation of concerns

## Verification Results

- `cargo build --release` compiles cleanly
- `cargo test` runs all 85 tests with 0 failures (9 new + 76 existing)
- `is_tool_allowed` called in both tools/list and tools/call paths in gateway.rs
- ping and initialize work without tool-level auth
- Viewer can list but not call, developer sees minus denied, admin sees all, unknown sees nothing

## Self-Check: PASSED
