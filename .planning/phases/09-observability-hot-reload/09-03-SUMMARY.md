---
phase: 09-observability-hot-reload
plan: 03
subsystem: gateway-integration
tags: [metrics, validation, hot-reload, sighup, prometheus]
dependency_graph:
  requires: [09-01, 09-02]
  provides: [full-observability, schema-validation, hot-config-reload]
  affects: [gateway, main, health-checker]
tech_stack:
  added: []
  patterns: [SharedHotConfig-RwLock, metrics-recording-all-paths, SIGHUP-hot-reload]
key_files:
  modified:
    - src/gateway.rs
    - src/main.rs
    - src/health/checker.rs
    - tests/gateway_integration_test.rs
    - tests/health_integration_test.rs
    - tests/stdio_integration.rs
decisions:
  - SharedHotConfig passed as Arc<RwLock<HotConfig>> to dispatch (read guard dropped after kill switch + rate limit checks)
  - Schema validation placed after RBAC but before circuit breaker in pipeline
  - Metrics passed as Option<Arc<Metrics>> for backward compatibility in tests
  - Health checker accepts Option<Arc<Metrics>> and calls set_backend_health after each check
  - SIGHUP handler runs in separate tokio::spawn (does not cancel on signal, loops forever)
metrics:
  duration: 8min
  completed: 2026-02-22
---

# Phase 9 Plan 03: Gateway Integration Summary

Wire metrics, schema validation, and hot config reload into gateway dispatch and main.rs -- completing Phase 9.

## What Was Done

### Task 1: Refactor gateway dispatch (8bbbba7)

Replaced `rate_limiter: &RateLimiter` and `kill_switch: &KillSwitchConfig` parameters with `hot_config: SharedHotConfig`. Added `metrics: Option<Arc<Metrics>>` and `schema_cache: &SchemaCache` parameters.

Pipeline order in `tools/call`:
1. Kill switch check (from `hot_config.read().await`)
2. Rate limit check (same read guard)
3. RBAC check (unchanged, not hot-reloadable)
4. Schema validation (NEW -- `schema_cache.validate()`, returns -32602 on failure)
5. Circuit breaker check (unchanged)
6. Backend dispatch (unchanged)

Metrics recorded on every path:
- `killed` -- kill switch rejection
- `rate_limited` -- rate limit hit (also records `rate_limit_hits_total`)
- `denied` -- RBAC rejection
- `invalid_args` -- schema validation failure
- `circuit_open` -- circuit breaker open
- `error` / `success` -- backend response

Updated all 4 integration test files to use new `run_dispatch` signature.

### Task 2: Wire into main.rs and health checker (cc05be5)

- Created `Arc<Metrics>` at startup, passed to health server (enables `/metrics` endpoint)
- Created `SchemaCache::from_catalog(&catalog)` for argument validation
- Created `SharedHotConfig` from `config.kill_switch` + `RateLimiter` (replaces standalone instances)
- Added SIGHUP handler: reloads `kill_switch` and `rate_limits` from config file atomically
- Passed metrics to health checker for `sentinel_backend_healthy` gauge updates
- Set initial backend health in metrics during startup

## Deviations from Plan

None -- plan executed exactly as written.

## Test Results

138 tests total, all passing:
- 33 unit tests (lib)
- 28 gateway integration tests
- 5 health integration tests
- 16 config tests
- 13 auth tests
- 11 MCP lifecycle tests
- 11 ID remap tests
- 8 backend tests
- 5 circuit breaker tests
- 5 catalog tests
- 3 stdio integration tests

## Phase 9 Requirements Satisfied

| ID | Requirement | Status |
|----|-------------|--------|
| OBS-01 | Prometheus metrics on /metrics endpoint | Complete |
| OBS-02 | Request count/latency/error metrics per tool | Complete |
| OBS-03 | Rate limit hit counter metric | Complete |
| OBS-04 | Backend health gauge metric | Complete |
| CONFIG-03 | SIGHUP hot reload of kill_switch + rate_limits | Complete |
| KILL-03 | Kill switch rejections recorded in metrics | Complete |

## Self-Check: PASSED

All 6 modified files exist. Both task commits (8bbbba7, cc05be5) verified in git log.
