---
phase: 06-rate-limiting-kill-switch
plan: 01
subsystem: rate-limiting
tags: [rate-limit, token-bucket, error-codes]
dependency_graph:
  requires: [config-types]
  provides: [ratelimit-module, rate-limit-error-codes]
  affects: [dispatch-loop]
tech_stack:
  added: []
  patterns: [token-bucket, lazy-refill, mutex-hashmap]
key_files:
  created:
    - src/ratelimit.rs
  modified:
    - src/protocol/jsonrpc.rs
    - src/lib.rs
decisions:
  - "std::sync::Mutex<HashMap> over DashMap -- single stdio transport means zero contention"
  - "Lazy refill on access (not background timer) -- zero resources when idle"
  - "Accept stale bucket accumulation for v1 (~24 bytes per unique client+tool pair)"
metrics:
  duration: 3min
  completed: 2026-02-22T05:14Z
---

# Phase 6 Plan 1: Token Bucket Rate Limiter Summary

In-memory token bucket rate limiter with per-client per-tool isolation and JSON-RPC error constants for rate limiting and kill switch responses.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add error code constants and error_with_data helper | 0e80ac1 | src/protocol/jsonrpc.rs |
| 2 | Implement RateLimiter module with unit tests | 73b5bab | src/ratelimit.rs, src/lib.rs |

## What Was Built

### Error Constants (jsonrpc.rs)
- `RATE_LIMIT_ERROR` (-32004) for rate-limited responses
- `KILL_SWITCH_ERROR` (-32005) for disabled tool/backend responses
- `error_with_data()` constructor on JsonRpcResponse for attaching retry-after metadata

### RateLimiter Module (ratelimit.rs)
- `TokenBucket` struct with lazy refill (resets to max after 60s window elapses)
- `RateLimiter` struct with `Mutex<HashMap<(String, String), TokenBucket>>` for per-client per-tool buckets
- Constructs from `RateLimitConfig` (default_rpm + per_tool overrides)
- `check(client, tool) -> Result<(), f64>` returns retry-after seconds on exhaustion
- 4 unit tests: within-limit, per-tool override, positive retry-after, client isolation

## Verification Results

- `cargo build --release` -- success, no warnings
- `cargo test --lib` -- 4/4 ratelimit tests pass
- RateLimiter exports publicly from lib.rs
- Error constants available at `protocol::jsonrpc::{RATE_LIMIT_ERROR, KILL_SWITCH_ERROR}`

## Deviations from Plan

None -- plan executed exactly as written.

## Self-Check: PASSED
