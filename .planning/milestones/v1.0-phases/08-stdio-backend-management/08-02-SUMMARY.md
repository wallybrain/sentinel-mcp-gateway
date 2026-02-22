---
phase: 08-stdio-backend-management
plan: 02
subsystem: backend
tags: [stdio, supervisor, crash-detection, backoff, mcp-handshake]
dependency_graph:
  requires: [StdioBackend, kill_process_group, drain_pending]
  provides: [run_supervisor, discover_stdio_tools]
  affects: [gateway, dispatch, catalog]
tech_stack:
  added: []
  patterns: [supervisor-loop, exponential-backoff-with-jitter, cancellation-token-select]
key_files:
  created: []
  modified:
    - src/backend/stdio.rs
    - src/backend/mod.rs
decisions:
  - Backoff jitter adds 0-50% of capped delay (prevents thundering herd on multi-backend restart)
  - Restart counter resets after 60s healthy operation (transient crashes don't cause permanent death)
  - MCP handshake failure triggers restart with backoff (same as crash path)
  - Tools channel sends backend clone for catalog/map updates
metrics:
  duration: 12min
  completed: 2026-02-22T06:51Z
---

# Phase 8 Plan 2: Supervisor and MCP Handshake Summary

Supervisor loop detects stdio child crashes via stdout EOF, restarts with exponential backoff (1s-60s + jitter), performs MCP tool discovery after each spawn, respects CancellationToken and max_restarts

## What Was Built

### discover_stdio_tools (65 lines)
- Sends MCP `initialize` request (JSON-RPC id=1) with protocol version and client info
- Sends `notifications/initialized` directly via `stdin_sender()` (no response expected)
- Sends `tools/list` request (JSON-RPC id=2) and parses `result.tools` array
- Returns `Vec<rmcp::model::Tool>` matching the HTTP backend pattern
- Added `stdin_sender()` accessor on StdioBackend for notification writes

### run_supervisor (120 lines)
- Supervisor loop with pre-spawn cancellation check
- Spawns child via `StdioBackend::spawn()`, performs MCP handshake via `discover_stdio_tools()`
- Monitors via `tokio::select!` on stdout_reader handle (child exit) vs `cancel.cancelled()` (shutdown)
- On crash: drains pending requests, kills process group, applies exponential backoff
- Backoff: `1s * 2^(n-1)` capped at 60s, plus random jitter (0-50% of delay)
- Backoff sleep uses `tokio::select!` vs cancel for prompt shutdown during wait
- Resets restart counter after 60s of healthy operation
- Stops after `max_restarts` reached (0 = unlimited)
- Sends `(name, tools, backend)` via channel after successful handshake

### backoff_delay helper
- Pure function: computes Duration from restart count, base, and max
- Uses `rand::random::<f64>()` for jitter

## Task Completion

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Implement discover_stdio_tools for MCP handshake | ee9a03e | src/backend/stdio.rs, src/backend/mod.rs |
| 2 | Implement supervisor with crash detection and backoff | 0d9cb12 | src/backend/stdio.rs, src/backend/mod.rs |

## Verification

- `cargo check` -- clean compile, no warnings
- `cargo test --lib backend::stdio` -- 6/6 tests passing
  - test_kill_process_group: existing (from Plan 01)
  - test_drain_pending: existing (from Plan 01)
  - test_spawn_and_send_with_cat: existing (from Plan 01)
  - test_supervisor_detects_child_exit_and_restarts: spawns "true" (exits immediately), verifies supervisor restarts and stops at max_restarts=3
  - test_supervisor_respects_cancellation_during_backoff: cancels during backoff sleep, verifies exit within 2s
  - test_supervisor_stops_after_max_restarts: max_restarts=2, verifies supervisor stops without external cancel

## Deviations from Plan

None - plan executed exactly as written.

## Decisions Made

1. **Backoff jitter 0-50% of capped delay** -- prevents synchronized restart storms when multiple backends crash simultaneously
2. **Restart counter resets after 60s** -- transient issues (brief network blip) don't accumulate toward permanent death
3. **MCP handshake failure = restart** -- if handshake fails, the backend is unusable; treat same as crash
4. **Channel sends backend clone** -- caller needs both tools and a live backend handle for the routing table
