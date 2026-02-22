---
phase: 01-foundation-config
plan: 02
subsystem: protocol
tags: [json-rpc, serde, atomic, concurrency, id-remapping]

requires:
  - phase: 01-foundation-config/01
    provides: Cargo project scaffold with lib.rs module structure
provides:
  - JSON-RPC 2.0 type system (JsonRpcId, JsonRpcRequest, JsonRpcResponse, JsonRpcError)
  - ID remapper for multiplexing client requests across backends
  - Error code constants matching JSON-RPC 2.0 spec
affects: [mcp-protocol, http-backend-routing, authentication, audit-logging]

tech-stack:
  added: []
  patterns: [serde untagged enum for polymorphic IDs, AtomicU64 + Mutex for thread-safe ID mapping, skip_serializing_if for JSON-RPC result/error exclusivity]

key-files:
  created:
    - src/protocol/jsonrpc.rs
    - src/protocol/id_remapper.rs
    - tests/id_remap_test.rs
  modified:
    - src/protocol/mod.rs

key-decisions:
  - "Own JSON-RPC types instead of using jsonrpc-core -- max control over serde behavior"
  - "AtomicU64 counter starting at 1 (not 0) for gateway IDs -- avoids null-like zero values"
  - "Default trait impl on IdRemapper for ergonomic construction"

patterns-established:
  - "TDD RED-GREEN for protocol types: tests first, then implementation"
  - "Integration tests in tests/ directory, not inline unit tests"
  - "Serde untagged for polymorphic JSON values"

requirements-completed: [PROTO-01, PROTO-04]

duration: 2min
completed: 2026-02-22
---

# Phase 1 Plan 2: JSON-RPC Types & ID Remapper Summary

**Custom JSON-RPC 2.0 type system with serde untagged ID enum and AtomicU64-based concurrent ID remapper proven collision-free across 1000 requests**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-22T02:13:22Z
- **Completed:** 2026-02-22T02:15:13Z
- **Tasks:** 1 (TDD with RED + GREEN phases)
- **Files modified:** 4

## Accomplishments
- JSON-RPC 2.0 types with polymorphic ID handling (number, string, null)
- Request/notification distinction via is_notification() method
- Response serialization with result XOR error (never both in JSON output)
- Thread-safe ID remapper with AtomicU64 counter and Mutex-guarded HashMap
- 11 tests covering serialization, deserialization, error codes, uniqueness, restore, cleanup, and 10-thread concurrency

## Task Commits

Each task was committed atomically (TDD split into RED + GREEN):

1. **Task 1 RED: Failing tests** - `4dc52a4` (test)
2. **Task 1 GREEN: Implementation** - `ec2c5bb` (feat)

## Files Created/Modified
- `src/protocol/jsonrpc.rs` - JsonRpcId, JsonRpcRequest, JsonRpcResponse, JsonRpcError, error code constants
- `src/protocol/id_remapper.rs` - IdRemapper with AtomicU64 counter and Mutex<HashMap> for concurrent ID remapping
- `src/protocol/mod.rs` - Module re-exports for jsonrpc and id_remapper
- `tests/id_remap_test.rs` - 11 integration tests for JSON-RPC types and ID remapper

## Decisions Made
- Own JSON-RPC types instead of using jsonrpc-core for maximum control over serde behavior
- AtomicU64 counter starts at 1 (not 0) to avoid null-like zero gateway IDs
- Added Default trait impl on IdRemapper for ergonomic construction
- Used Ordering::Relaxed for counter -- uniqueness guaranteed by fetch_add atomicity regardless of ordering

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- JSON-RPC types ready for Phase 2 MCP protocol layer
- ID remapper ready for HTTP backend routing (Phase 3)
- All 19 tests passing (8 config + 11 protocol), zero clippy warnings, clean release build

---
*Phase: 01-foundation-config*
*Completed: 2026-02-22*
