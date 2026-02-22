---
phase: 03-http-backend-routing
plan: 01
subsystem: backend
tags: [http, sse, retry, reqwest, backend]
dependency_graph:
  requires: [config-types]
  provides: [http-backend, sse-parser, backend-error, retry-logic]
  affects: [dispatch-loop, backend-routing]
tech_stack:
  added: [reqwest-0.12, bytes-1, futures-0.3, rand-0.9]
  patterns: [exponential-backoff-jitter, sse-parsing, connection-pooling]
key_files:
  created:
    - src/backend/mod.rs
    - src/backend/http.rs
    - src/backend/sse.rs
    - src/backend/retry.rs
    - src/backend/error.rs
    - tests/backend_test.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/lib.rs
decisions:
  - reqwest 0.12 instead of 0.13 (rustls-tls feature not available in 0.13)
metrics:
  duration: 5min
  completed: 2026-02-22T03:25Z
  tasks: 2/2
  tests_added: 13
  tests_total: 54
---

# Phase 3 Plan 1: HTTP Backend Module Summary

**reqwest-based HTTP backend with SSE parsing, exponential backoff retry, and typed error classification**

## What Was Built

### BackendError (`src/backend/error.rs`)
Enum with 5 variants: `Request`, `HttpStatus`, `Stream`, `NoDataInSse`, `InvalidResponse`. The `is_retryable()` method classifies timeout/connect errors and 5xx as retryable; 4xx and parse errors as permanent failures.

### SSE Parser (`src/backend/sse.rs`)
`parse_sse_data(raw: &str) -> Option<String>` extracts the first non-empty `data:` line from SSE-formatted text. Handles edge cases: missing data lines, empty data, extra whitespace.

### Retry Logic (`src/backend/retry.rs`)
`retry_with_backoff` generic async function with configurable max retries. Base delay: 100ms * 2^attempt. Jitter: random 0..base/2. Logs each retry with tracing::warn. Zero retries means single attempt.

### HttpBackend (`src/backend/http.rs`)
- `build_http_client()` creates a shared reqwest::Client with TCP nodelay, connection pooling (10 idle per host, 90s timeout), 5s connect timeout
- `HttpBackend::new()` takes a shared Client + BackendConfig, auto-appends `/mcp` to URL
- `HttpBackend::send()` POSTs JSON-RPC with retry, detects SSE content-type for streaming responses
- `read_sse_response()` accumulates byte stream chunks and parses SSE data lines

### Test Suite (`tests/backend_test.rs`)
13 tests covering SSE parsing (5), error classification (4), URL construction (3), client building (1).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] reqwest version downgraded from 0.13 to 0.12**
- **Found during:** Task 1
- **Issue:** reqwest 0.13 does not have the `rustls-tls` feature. The plan specified `reqwest = "0.13"`.
- **Fix:** Changed to `reqwest = "0.12"` which has all required features (rustls-tls, json, stream).
- **Files modified:** Cargo.toml
- **Commit:** a8d67f5

**2. [Rule 3 - Blocking] Manual Display/Error impl instead of thiserror derive**
- **Found during:** Task 1
- **Issue:** BackendError wraps `reqwest::Error` which doesn't implement `Clone`, and thiserror derive macros have constraints that make mixed-source enums verbose. Manual impl is cleaner.
- **Fix:** Implemented `Display` and `Error` traits manually for BackendError.
- **Files modified:** src/backend/error.rs
- **Commit:** a8d67f5

## Commits

| Hash | Type | Description |
|------|------|-------------|
| a8d67f5 | feat | HTTP backend module with SSE parser, retry, and error types |
| fe2c1b3 | chore | Update Cargo.lock with new dependencies |
| 0a2b03a | test | 13 unit tests for HTTP backend module |

## Self-Check: PASSED

- [x] src/backend/mod.rs exists
- [x] src/backend/http.rs exists
- [x] src/backend/sse.rs exists
- [x] src/backend/retry.rs exists
- [x] src/backend/error.rs exists
- [x] tests/backend_test.rs exists
- [x] Commit a8d67f5 exists
- [x] Commit fe2c1b3 exists
- [x] Commit 0a2b03a exists
- [x] 54 total tests passing (41 existing + 13 new)
- [x] cargo clippy: 0 warnings
- [x] cargo build --release: success
