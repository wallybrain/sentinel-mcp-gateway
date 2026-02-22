---
phase: 09-observability-hot-reload
plan: 02
subsystem: validation, config
tags: [schema-validation, hot-reload, json-schema, config-reload]
dependency_graph:
  requires: [catalog, config, ratelimit]
  provides: [SchemaCache, HotConfig, SharedHotConfig, reload_hot_config]
  affects: [gateway dispatch (Plan 03)]
tech_stack:
  added: [jsonschema 0.42]
  patterns: [Arc<RwLock<>> for atomic config swap, HashMap<String, Validator> for compiled schemas]
key_files:
  created:
    - src/validation/mod.rs
    - src/config/hot.rs
  modified:
    - src/lib.rs
    - src/config/mod.rs
    - src/config/types.rs
    - Cargo.toml
decisions:
  - jsonschema 0.42 for JSON Schema validation (instance_path() is a method, not field)
  - RateLimitConfig import scoped to #[cfg(test)] to avoid unused import warning in lib build
  - Clone derive added to KillSwitchConfig for HotConfig creation from parsed config
metrics:
  duration: 6min
  completed: 2026-02-22T07:46Z
  tasks: 2/2
  tests_added: 8
  total_tests: 33
---

# Phase 9 Plan 2: Schema Validation & Hot Config Summary

JSON schema validation for tool arguments via jsonschema 0.42, plus HotConfig struct for atomic SIGHUP-triggered kill switch and rate limit reload.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | SchemaCache module for JSON Schema validation | 4b5ae7a | src/validation/mod.rs, src/lib.rs, Cargo.toml |
| 2 | HotConfig struct and reload function | 55fd5c5 | src/config/hot.rs, src/config/mod.rs, src/config/types.rs |

## What Was Built

### SchemaCache (src/validation/mod.rs)
- `SchemaCache::from_catalog(&ToolCatalog)` compiles JSON Schema validators from tool input schemas
- `SchemaCache::validate(tool_name, arguments)` returns descriptive errors with field paths
- Gracefully skips tools with invalid/missing schemas (warn + skip, never crash)
- 5 unit tests: valid args, invalid type, missing required, unknown tool, invalid schema

### HotConfig (src/config/hot.rs)
- `HotConfig` bundles `KillSwitchConfig` + `RateLimiter` for atomic swap
- `SharedHotConfig` type alias: `Arc<RwLock<HotConfig>>`
- `reload_hot_config(path)` re-reads sentinel.toml, returns new HotConfig or error (caller keeps previous on failure)
- Only reloads kill_switch and rate_limits -- backend/auth changes require restart (by design)
- 3 unit tests: creation, valid reload from temp file, invalid file path

## Deviations from Plan

None - plan executed exactly as written.

## Decisions Made

1. **jsonschema 0.42 API**: `instance_path()` is a method call (not a field) in this version -- plan referenced it as a field
2. **RateLimitConfig import scoping**: Moved to `#[cfg(test)]` block to avoid unused import warning in non-test builds
3. **Clone on KillSwitchConfig**: Added as specified in plan -- needed for HotConfig creation from parsed config

## Verification

- `cargo test --lib validation` -- 5/5 pass
- `cargo test --lib config::hot` -- 3/3 pass
- `cargo test --lib` -- 33/33 pass (full lib)
- `cargo build` -- compiles without warnings
- Both structs confirmed present via grep
