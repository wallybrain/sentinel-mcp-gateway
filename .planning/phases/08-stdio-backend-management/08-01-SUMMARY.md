---
phase: 08-stdio-backend-management
plan: 01
subsystem: backend
tags: [stdio, process-management, multiplexing, json-rpc]
dependency_graph:
  requires: []
  provides: [StdioBackend, kill_process_group, drain_pending]
  affects: [gateway, dispatch]
tech_stack:
  added: [nix-0.29]
  patterns: [process-group-spawn, request-id-correlation, oneshot-channels]
key_files:
  created:
    - src/backend/stdio.rs
  modified:
    - Cargo.toml
    - src/backend/error.rs
    - src/backend/mod.rs
decisions:
  - std::sync::Mutex for pending map (matches project pattern, zero contention)
  - process_group(0) + kill_on_drop(false) for explicit lifecycle management
  - Individual env() calls to preserve parent environment inheritance
  - Bounded mpsc channel (64) for stdin writes
metrics:
  duration: 3min
  completed: 2026-02-22T06:36Z
---

# Phase 8 Plan 1: StdioBackend Core Summary

StdioBackend spawns child processes in own process group, multiplexes concurrent JSON-RPC via request ID correlation with oneshot channels, kills via nix::killpg

## What Was Built

### StdioBackend struct (329 lines)
- `spawn()` -- creates child process with piped stdin/stdout/stderr, process_group(0), returns backend + task handles
- `send()` -- inserts oneshot sender into pending map, writes to stdin channel, awaits response with timeout
- `name()` -- returns backend name for logging (parallels HttpBackend::url())
- `pid()` -- returns current child PID via AtomicU32

### Supporting functions
- `kill_process_group(pid)` -- sends SIGTERM via nix::killpg, handles ESRCH gracefully
- `drain_pending()` -- drains all pending oneshot senders on process exit
- `stdin_writer` task -- reads from mpsc channel, writes lines to child stdin with flush
- `stdout_reader` task -- reads lines from child stdout, parses JSON for id field, routes to pending map

### BackendError extensions
- `ProcessExited(String)` -- child process exited unexpectedly (not retryable)
- `StdinClosed` -- stdin channel closed (not retryable)

## Task Completion

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add nix dependency and extend BackendError | cc68903 | Cargo.toml, src/backend/error.rs |
| 2 | Implement StdioBackend with multiplexer | 83c6130 | src/backend/stdio.rs, src/backend/mod.rs |

## Verification

- `cargo check` -- clean compile, no warnings
- `cargo test --lib backend::stdio` -- 3/3 tests passing
  - test_kill_process_group: spawns sleep, kills process group, verifies exit
  - test_drain_pending: inserts senders, drains, verifies receivers get errors
  - test_spawn_and_send_with_cat: spawns cat, sends JSON-RPC, verifies echo response

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed mutable receiver for oneshot::try_recv**
- **Found during:** Task 2 test compilation
- **Issue:** `try_recv()` requires `&mut self` on tokio oneshot receiver
- **Fix:** Added `mut` binding to rx1, rx2 in test_drain_pending
- **Files modified:** src/backend/stdio.rs

## Decisions Made

1. **std::sync::Mutex for pending map** -- matches project pattern from rate limiter; pending map operations are trivial lock-unlock with zero contention
2. **process_group(0) + kill_on_drop(false)** -- explicit lifecycle management; gateway controls when processes die
3. **Individual env() calls** -- preserves parent environment inheritance (envs() replaces all inherited env vars)
4. **Bounded mpsc(64) for stdin** -- backpressure if child process stalls reading stdin
