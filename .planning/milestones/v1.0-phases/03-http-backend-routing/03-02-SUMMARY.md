---
phase: 03-http-backend-routing
plan: 02
subsystem: gateway-dispatch
tags: [routing, dispatch, tools-call, id-remapping, discovery]
dependency_graph:
  requires: [http-backend, sse-parser, tool-catalog, id-remapper]
  provides: [tools-call-routing, backend-discovery, dispatch-with-backends]
  affects: [main-startup, gateway-dispatch-loop]
tech_stack:
  added: []
  patterns: [catalog-routing, id-remap-restore, graceful-fallback, mcp-handshake-discovery]
key_files:
  created: []
  modified:
    - src/gateway.rs
    - src/main.rs
    - src/backend/http.rs
    - src/backend/mod.rs
    - src/protocol/jsonrpc.rs
    - tests/gateway_integration_test.rs
decisions:
  - Stub catalog fallback when no HTTP backends are reachable (ensures binary always starts)
  - discover_tools lives in backend/http.rs (collocated with HttpBackend)
  - fire-and-forget notifications/initialized during discovery (some backends may not respond)
metrics:
  duration: 4min
  completed: 2026-02-22T03:31Z
  tasks: 2/2
  tests_added: 6
  tests_total: 60
---

# Phase 3 Plan 2: Tools/Call Dispatch & Backend Discovery Summary

**tools/call routing through HttpBackend with ID remapping plus MCP handshake-based tool discovery at startup**

## What Was Built

### tools/call Routing (`src/gateway.rs`)
New `tools/call` match arm in the dispatch loop. Extracts tool name from `params.name`, routes via `catalog.route()` to find the backend, remaps the client's JSON-RPC ID before sending to the backend, and restores the original ID on the response. Three error paths: missing name (INVALID_PARAMS), unknown tool (INVALID_PARAMS), backend unavailable/error (INTERNAL_ERROR).

Helper function `handle_tools_call()` encapsulates all routing logic: param extraction, catalog lookup, backend lookup, ID remapping, HTTP dispatch, response parsing, and error recovery.

### Backend Discovery (`src/backend/http.rs`)
`discover_tools()` performs the full MCP handshake against an HTTP backend: initialize (with protocolVersion 2025-03-26) -> notifications/initialized (fire and forget) -> tools/list. Extracts `result.tools` from the response and returns `Vec<Tool>`.

### Startup Wiring (`src/main.rs`)
Replaced stub catalog with live backend discovery. Builds a shared reqwest::Client, iterates HTTP backends from config, discovers tools from each, registers them in the catalog, and stores backends in a HashMap. Falls back to stub catalog if no backends are reachable. Passes backends HashMap and IdRemapper to the dispatch loop.

### Updated Dispatch Signature
`run_dispatch` now accepts `&HashMap<String, HttpBackend>` and `&IdRemapper` in addition to the catalog. `JsonRpcResponse` gained `Deserialize` derive for parsing backend responses.

### Test Suite (`tests/gateway_integration_test.rs`)
6 new tests: unknown tool returns INVALID_PARAMS, missing name returns INVALID_PARAMS, no params returns INVALID_PARAMS, catalog routes to correct backend, backend not in map returns INTERNAL_ERROR, ID remapper round-trip. All 6 existing tests updated to new signature and still pass.

## Deviations from Plan

None - plan executed exactly as written.

## Commits

| Hash | Type | Description |
|------|------|-------------|
| 5c84b44 | feat | Wire tools/call routing into dispatch loop with backend discovery |
| 673d225 | test | Integration tests for tools/call dispatch and error paths |

## Self-Check: PASSED

- [x] src/gateway.rs exists (modified)
- [x] src/main.rs exists (modified)
- [x] src/backend/http.rs exists (modified)
- [x] src/backend/mod.rs exists (modified)
- [x] src/protocol/jsonrpc.rs exists (modified)
- [x] tests/gateway_integration_test.rs exists (modified)
- [x] Commit 5c84b44 exists
- [x] Commit 673d225 exists
- [x] 60 total tests passing (54 existing + 6 new)
- [x] cargo clippy --all-targets: 0 warnings
- [x] cargo build --release: success
