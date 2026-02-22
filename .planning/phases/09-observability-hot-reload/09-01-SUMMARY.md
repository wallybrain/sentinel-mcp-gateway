---
phase: 09-observability-hot-reload
plan: 01
subsystem: metrics
tags: [prometheus, observability, metrics, health-server]
dependency_graph:
  requires: []
  provides: [metrics-module, metrics-endpoint]
  affects: [health-server, main]
tech_stack:
  added: [prometheus 0.14, http-body-util 0.1]
  patterns: [explicit-registry, optional-state, clone-send-sync-metrics]
key_files:
  created:
    - src/metrics/mod.rs
  modified:
    - src/health/server.rs
    - src/main.rs
    - Cargo.toml
decisions:
  - Explicit prometheus::Registry (not default_registry) for test isolation
  - HealthAppState struct combines BackendHealthMap + Option<Arc<Metrics>> for axum state
  - Metrics are Option in health router -- None returns 404, Some returns prometheus text
metrics:
  duration: 6min
  completed: 2026-02-22T07:46Z
  tasks: 2/2
  files: 4
---

# Phase 9 Plan 01: Prometheus Metrics Module Summary

Prometheus metrics module with 5 metric families (sentinel_* prefix) and /metrics endpoint on the health server (port 9201).

## What Was Built

### Task 1: Metrics Module (`src/metrics/mod.rs`)

`Metrics` struct with explicit registry containing:
- `sentinel_requests_total` (CounterVec) -- labels: tool, status
- `sentinel_request_duration_seconds` (HistogramVec) -- labels: tool
- `sentinel_errors_total` (CounterVec) -- labels: tool, error_type
- `sentinel_backend_healthy` (GaugeVec) -- labels: backend
- `sentinel_rate_limit_hits_total` (CounterVec) -- labels: tool

Helper methods: `record_request()`, `record_rate_limit_hit()`, `set_backend_health()`, `gather_text()`.

4 unit tests all passing.

### Task 2: /metrics Endpoint (`src/health/server.rs`)

- `HealthAppState` struct with `health_map` + `Option<Arc<Metrics>>`
- `/metrics` route returns Prometheus text format when enabled, 404 when None
- All existing health/readiness handlers updated to use new state struct
- main.rs passes `None` (wiring deferred to plan 03)
- 2 new tests: enabled (200 with sentinel_ content) and disabled (404)

## Test Results

- `cargo test --lib metrics` -- 4 passed
- `cargo test --lib health` -- 12 passed (6 existing + 2 new metrics + 4 circuit breaker)
- `cargo build` -- no warnings

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1 | 92a8432 | Metrics module with registry, definitions, helpers, tests |
| 2 | e1ea8f3 | /metrics endpoint on health server with HealthAppState |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed jsonschema instance_path() API call**
- **Found during:** Task 1 (build)
- **Issue:** `src/validation/mod.rs` used `error.instance_path` (field access) but jsonschema 0.42 requires `error.instance_path()` (method call)
- **Fix:** Added parentheses for method call
- **Files modified:** src/validation/mod.rs
- **Commit:** 92a8432

**2. [Rule 3 - Blocking] Added http-body-util dev dependency**
- **Found during:** Task 2 (tests)
- **Issue:** Response body collection in tests requires `BodyExt` from http-body-util
- **Fix:** Added `http-body-util = "0.1"` to dev-dependencies
- **Files modified:** Cargo.toml
- **Commit:** e1ea8f3

**3. [Rule 1 - Bug] Fixed test for unobserved metrics**
- **Found during:** Task 1 (tests)
- **Issue:** Prometheus only includes metric families in gather output after at least one observation; test expected all 5 families without recording any values
- **Fix:** Touch each metric in test before asserting presence
- **Files modified:** src/metrics/mod.rs
- **Commit:** 92a8432

## Self-Check: PASSED

- src/metrics/mod.rs: FOUND
- src/health/server.rs: FOUND
- Commit 92a8432: FOUND
- Commit e1ea8f3: FOUND
