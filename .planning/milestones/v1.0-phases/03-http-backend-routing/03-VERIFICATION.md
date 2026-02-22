---
phase: 03-http-backend-routing
verified: 2026-02-22T04:10:00Z
status: gaps_found
score: 9/11 must-haves verified
re_verification: false
gaps:
  - truth: "SSE (text/event-stream) responses from backends stream through the gateway without buffering"
    status: partial
    reason: "read_sse_response() accumulates ALL chunks into a BytesMut buffer before parsing. The ROADMAP success criterion says 'without buffering' but the implementation does full-buffer accumulation. This works correctly for MCP (single-event SSE streams) but is technically buffered, not streamed."
    artifacts:
      - path: "src/backend/http.rs"
        issue: "Lines 110-121: read_sse_response accumulates into BytesMut before calling parse_sse_data"
    missing:
      - "Decide whether to update the success criterion to reflect 'accumulate SSE to extract data line' (accurate) or implement true streaming passthrough (architecturally unnecessary for stdio→stdio gateway)"
  - truth: "REQUIREMENTS.md traceability table reflects completed status for ROUTE-03 and ROUTE-04"
    status: failed
    reason: "ROUTE-03 (connection pooling/timeouts) and ROUTE-04 (retry with backoff) are fully implemented in src/backend/http.rs and src/backend/retry.rs respectively, but REQUIREMENTS.md shows both as '[ ]' (unchecked) and 'Pending' in the traceability table."
    artifacts:
      - path: ".planning/REQUIREMENTS.md"
        issue: "Line 26: ROUTE-03 marked '[ ]' (should be '[x]'). Line 27: ROUTE-04 marked '[ ]' (should be '[x]'). Lines 151-152: both show 'Pending' in traceability table."
    missing:
      - "Update REQUIREMENTS.md: mark ROUTE-03 and ROUTE-04 as '[x]' and change traceability status from 'Pending' to 'Complete'"
human_verification:
  - test: "End-to-end tool call with real n8n backend"
    expected: "A tools/call request for an n8n tool reaches the n8n MCP server and returns the correct response"
    why_human: "Integration tests use stub catalog with empty backends map; no test exercises a real HTTP backend. Requires n8n running at configured URL."
  - test: "End-to-end tool call with real sqlite backend"
    expected: "A tools/call request for a sqlite tool reaches the sqlite MCP server and returns the correct response"
    why_human: "Same as above — no test exercises a real sqlite MCP server."
  - test: "Retry behavior under transient failure"
    expected: "A backend timeout triggers automatic retry with exponential backoff; tracing::warn log shows retry attempts"
    why_human: "retry_with_backoff is unit-tested structurally but no test injects a transient HTTP failure to verify the full retry loop fires in the live gateway."
---

# Phase 3: HTTP Backend Routing Verification Report

**Phase Goal:** Tool calls route to real HTTP backends (n8n, sqlite) with reliable connection handling and streaming support
**Verified:** 2026-02-22T04:10:00Z
**Status:** gaps_found (2 gaps: 1 partial, 1 documentation)
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

Plan 01 must-haves:

| # | Truth | Status | Evidence |
|---|-------|--------|---------|
| 1 | HttpBackend can POST JSON-RPC to an HTTP endpoint and parse SSE responses | VERIFIED | `src/backend/http.rs` lines 64-121: `send()` POSTs with Content-Type/Accept headers; `read_sse_response()` handles SSE content-type |
| 2 | Transient errors trigger automatic retry with exponential backoff and jitter | VERIFIED | `src/backend/retry.rs`: `retry_with_backoff` with `100ms * 2^attempt` base, `rand::rng().random_range(0..base/2)` jitter, `is_retryable()` guard |
| 3 | Non-retryable errors (4xx, invalid response) fail immediately without retry | VERIFIED | `BackendError::is_retryable()`: HttpStatus <500 returns false, NoDataInSse returns false, InvalidResponse returns false |
| 4 | SSE data: lines are correctly parsed into JSON-RPC response strings | VERIFIED | `src/backend/sse.rs`: `parse_sse_data()` strips `data:` prefix, trims, skips empty; 5 unit tests pass |
| 5 | Connection pooling is configured via shared reqwest::Client | VERIFIED | `build_http_client()`: `pool_max_idle_per_host(10)`, `pool_idle_timeout(90s)`, `connect_timeout(5s)`, `tcp_nodelay(true)` |

Plan 02 must-haves:

| # | Truth | Status | Evidence |
|---|-------|--------|---------|
| 6 | A tools/call request for an n8n tool routes to the n8n backend via HttpBackend | PARTIAL | Dispatch logic wired and unit-tested; no test with real n8n backend (deferred to Phase 10 per plan) |
| 7 | A tools/call request for a sqlite tool routes to the sqlite backend via HttpBackend | PARTIAL | Same — routing logic correct, no live backend test |
| 8 | Response from backend has its JSON-RPC ID restored to the client's original ID | VERIFIED | `gateway.rs` lines 204-209: `id_remapper.restore(gateway_id)` applied to `backend_resp.id`; round-trip test passes |
| 9 | Unknown tool names return INVALID_PARAMS code | VERIFIED | `handle_tools_call()` returns `INVALID_PARAMS` for missing name and unknown tool; 3 integration tests pass |
| 10 | Backend communication errors return INTERNAL_ERROR | VERIFIED | `gateway.rs` lines 226-238: `BackendError` → `INTERNAL_ERROR`; `test_tools_call_backend_not_in_map_returns_internal_error` passes |
| 11 | Tool catalog populated from real HTTP backends at startup (replacing stub) | VERIFIED | `main.rs` lines 44-86: `build_http_client()` → `discover_tools()` → `catalog.register_backend()` → `backends_map.insert()`; falls back to stub if discovery fails |

**Score:** 9/11 truths verified (2 partial — live backend tests deferred, which the plan explicitly documents)

### Required Artifacts

Plan 01:

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/backend/mod.rs` | Module re-exports for BackendError, HttpBackend, build_http_client | VERIFIED | Exports: BackendError, HttpBackend, build_http_client, discover_tools, parse_sse_data (9 lines) |
| `src/backend/http.rs` | HttpBackend struct with send() method (min 50 lines) | VERIFIED | 183 lines; HttpBackend struct with send(), read_sse_response(), url(), new(); build_http_client(); discover_tools() |
| `src/backend/sse.rs` | SSE line parser extracting data: content (min 15 lines) | VERIFIED | 15 lines; parse_sse_data() iterates lines, strips prefix, trims, skips empty |
| `src/backend/retry.rs` | retry_with_backoff generic async function (min 25 lines) | VERIFIED | 51 lines; generic over F: FnMut() -> Fut; exponential backoff + jitter; tracing::warn per retry |
| `src/backend/error.rs` | BackendError enum with is_retryable() (min 25 lines) | VERIFIED | 46 lines; 5 variants; is_retryable() correct; manual Display + Error impl |
| `tests/backend_test.rs` | Unit tests for SSE parsing, retry, error classification (min 50 lines) | VERIFIED | 117 lines; 13 tests: 5 SSE, 4 error, 3 URL construction, 1 client build; all pass |

Plan 02:

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/gateway.rs` | Dispatch loop with tools/call routing + ID remapping | VERIFIED | 248 lines; `tools/call` arm calls `handle_tools_call()`; ID remap/restore wired |
| `src/main.rs` | Startup wiring: build HTTP client, discover backends, populate catalog | VERIFIED | 109 lines; contains build_http_client, discover_tools, catalog.register_backend, backends_map |
| `tests/gateway_integration_test.rs` | Integration tests for tools/call dispatch | VERIFIED | 333 lines; 12 tests: 6 existing + 6 new tools/call tests; all pass |

### Key Link Verification

Plan 01:

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/backend/http.rs` | `src/backend/sse.rs` | `read_sse_response` calls `parse_sse_data` | WIRED | Line 120: `parse_sse_data(&raw).ok_or(BackendError::NoDataInSse)` |
| `src/backend/http.rs` | `src/backend/retry.rs` | `send()` wraps POST in `retry_with_backoff` | WIRED | Line 70: `retry_with_backoff(self.max_retries, move \|\| { ... })` |
| `src/backend/http.rs` | `src/backend/error.rs` | All errors are BackendError variants | WIRED | Lines 84, 89, 115, 120: all return `BackendError::*` variants |

Plan 02:

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/gateway.rs` | `src/backend/http.rs` | Dispatch loop calls `HttpBackend::send()` | WIRED | Line 201: `backend.send(&body).await` inside `handle_tools_call()` |
| `src/gateway.rs` | `src/protocol/id_remapper.rs` | Remap before send, restore after receive | WIRED | Line 189: `id_remapper.remap(client_id, &backend_name)`; Line 206: `id_remapper.restore(gateway_id)` |
| `src/gateway.rs` | `src/catalog/mod.rs` | `catalog.route(tool_name)` to find backend | WIRED | Line 160: `catalog.route(&tool_name)` |
| `src/main.rs` | `src/backend/http.rs` | Build shared client, create HttpBackend per config, discover tools | WIRED | Lines 45-59: `build_http_client()`, `HttpBackend::new()`, `discover_tools()` |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|---------|
| ROUTE-01 | 03-02-PLAN.md | Gateway routes tools/call to correct HTTP backend | SATISFIED | `handle_tools_call()` in gateway.rs: catalog.route() → backends.get() → backend.send() |
| ROUTE-03 | 03-01-PLAN.md | Connection pooling, keep-alive, configurable timeouts | SATISFIED (code) / PENDING (docs) | `build_http_client()`: pool_max_idle_per_host(10), pool_idle_timeout(90s), connect_timeout(5s). **REQUIREMENTS.md not updated.** |
| ROUTE-04 | 03-01-PLAN.md | Retry failed HTTP requests with exponential backoff + jitter | SATISFIED (code) / PENDING (docs) | `retry_with_backoff()` in retry.rs; wired into `send()` in http.rs. **REQUIREMENTS.md not updated.** |
| PROTO-05 | 03-01-PLAN.md | Gateway proxies SSE responses from backends | PARTIAL | SSE detected by content-type and parsed via bytes_stream(); full buffer accumulated before data extraction — not true streaming passthrough. Functional for MCP single-event SSE. |

**Orphaned requirements:** None. All 4 IDs appear in plan frontmatter.

**Documentation gap:** REQUIREMENTS.md checkbox `[ ]` and traceability `Pending` for ROUTE-03 and ROUTE-04 — implementation is complete but the requirements file was not updated.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/backend/http.rs` | 112 | `BytesMut` accumulation of SSE stream | Warning | Full SSE response buffered in memory before parsing; adequate for current MCP use case but violates ROADMAP SC #3 ("without buffering") |

No TODO/FIXME/placeholder comments found. No empty return null/empty implementations found. No console.log-only implementations (Rust project, tracing used appropriately).

### Human Verification Required

#### 1. Real n8n Backend Tool Call

**Test:** Configure sentinel.toml with the n8n MCP backend URL, start the gateway, send a valid `tools/call` JSON-RPC message via stdin for a known n8n tool
**Expected:** The gateway performs the MCP handshake with n8n at startup (visible in tracing logs), the tool call routes to n8n, and the JSON-RPC response is returned on stdout with the client's original ID
**Why human:** No test exercises a real HTTP backend; integration tests use an empty backends HashMap. Real backend tests are deferred to Phase 10 by plan design.

#### 2. Real sqlite Backend Tool Call

**Test:** Same as above, using the sqlite MCP backend
**Expected:** `read_query` or `write_query` tool routes to sqlite, returns result
**Why human:** Same reason — no live backend test exists.

#### 3. Retry Under Transient Failure

**Test:** Start the gateway with a backend configured to an unreachable URL (or one that returns 503), send a `tools/call` request
**Expected:** Tracing logs show `"retrying after transient error"` with attempt numbers; after max_retries exhausted, client receives INTERNAL_ERROR response
**Why human:** retry_with_backoff is unit-tested for the retry decision logic, but the end-to-end path (gateway receives request → backend fails → retry fires → INTERNAL_ERROR returned to client) is not tested.

### Gaps Summary

Two gaps found:

**Gap 1 (Warning — SSE buffering):** The ROADMAP success criterion says SSE responses "stream through without buffering." The implementation fully buffers the SSE response in `BytesMut` before extracting the JSON-RPC data line. This works correctly for MCP backends that return a single SSE event per JSON-RPC request, but is technically not streaming passthrough. Resolution: either update the ROADMAP criterion to accurately describe the actual behavior (accumulate-then-parse), or implement incremental SSE line parsing that can return on first data line found (avoiding waiting for stream close). The latter would be a minor refactor of `read_sse_response`.

**Gap 2 (Documentation — REQUIREMENTS.md not updated):** ROUTE-03 and ROUTE-04 are fully implemented in `src/backend/http.rs` and `src/backend/retry.rs`, verified by 4 unit tests in `tests/backend_test.rs` and compiling build. However, `REQUIREMENTS.md` still shows `[ ] ROUTE-03` and `[ ] ROUTE-04` with traceability status "Pending." These should be `[x]` and "Complete." This is a documentation gap, not a functional gap.

**Root cause for both gaps:** Minor process gaps — SSE criterion was written aspirationally (true streaming passthrough), implementation chose simpler accumulate-and-parse approach; REQUIREMENTS.md was not updated after plan completion (ROUTE-01 and PROTO-05 were updated but ROUTE-03/04 were missed).

---

_Verified: 2026-02-22T04:10:00Z_
_Verifier: Claude (gsd-verifier)_
