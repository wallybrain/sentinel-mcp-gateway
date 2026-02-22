---
phase: 08-stdio-backend-management
verified: 2026-02-22T07:15:00Z
status: passed
score: 14/14 must-haves verified
---

# Phase 8: Stdio Backend Management Verification Report

**Phase Goal:** The gateway governs stdio-based MCP servers (context7, firecrawl, exa, playwright, sequential-thinking) -- the unique differentiator
**Verified:** 2026-02-22T07:15:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A StdioBackend can spawn a child process with piped stdin/stdout in its own process group | VERIFIED | `process_group(0)` at line 40 of stdio.rs, `Command::new` with piped stdin/stdout/stderr |
| 2 | Multiple concurrent JSON-RPC requests to the same StdioBackend are multiplexed via request ID correlation | VERIFIED | `pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>` with insert/remove in send(), stdout_reader routes by id |
| 3 | A StdioBackend can kill its entire process group (not just direct child) | VERIFIED | `kill_process_group()` calls `nix::sys::signal::killpg(pgid, Signal::SIGTERM)` at line 288 |
| 4 | Requests that timeout are cleaned up from the pending map | VERIFIED | Timeout branch in send() calls `map.remove(&id)` at line 115 |
| 5 | A crashed stdio backend is detected when its stdout reader hits EOF | VERIFIED | stdout_reader breaks on `Ok(0)` (EOF), supervisor monitors via `stdout_handle` in tokio::select |
| 6 | A crashed stdio backend is restarted with exponential backoff (base 1s, max 60s, jitter) | VERIFIED | `backoff_delay()` function computes `base * 2^(n-1)` capped at 60s with 0-50% jitter, 6/6 unit tests pass |
| 7 | Restart respects max_restarts limit from config | VERIFIED | `config.max_restarts > 0 && restart_count >= config.max_restarts` check at lines 333, 372, 406 |
| 8 | Supervisor stops cleanly when CancellationToken is cancelled | VERIFIED | `cancel.is_cancelled()` before spawn, `cancel.cancelled()` in select during monitor and backoff sleep |
| 9 | MCP handshake (initialize + tools/list) runs after each spawn to discover tools | VERIFIED | `discover_stdio_tools()` sends initialize (id=1), notifications/initialized, tools/list (id=2), returns Vec<Tool> |
| 10 | A tools/call request for a stdio-backed tool routes correctly and returns the response | VERIFIED | Integration test `test_stdio_tools_call_through_dispatch` sends tools/call through full dispatch loop, gets "hello from dispatch" back |
| 11 | tools/list includes tools from both HTTP and stdio backends | VERIFIED | gateway.rs `handle_tools_call` and tools/list use `HashMap<String, Backend>`, catalog registers both types |
| 12 | Stdio backends are spawned on gateway startup from config | VERIFIED | main.rs filters `BackendType::Stdio`, spawns `run_supervisor` for each, awaits tool discovery with 30s timeout |
| 13 | On gateway shutdown, all stdio child processes are terminated via process group kill | VERIFIED | main.rs `cancel.cancel()` triggers supervisor shutdown which calls `kill_process_group(pid)`, 5s timeout per supervisor handle |
| 14 | The dispatch loop handles both HTTP and stdio backends transparently | VERIFIED | `Backend` enum with `Http`/`Stdio` variants, `send()` delegates via match, gateway.rs uses `Backend` not `HttpBackend` |

**Score:** 14/14 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/backend/stdio.rs` | StdioBackend, supervisor, discover_stdio_tools (>150 lines) | VERIFIED | 630 lines, contains spawn, send, supervisor, MCP handshake, kill, drain |
| `src/backend/error.rs` | BackendError with ProcessExited, StdinClosed | VERIFIED | Both variants present with Display, is_retryable (false), Error::source |
| `Cargo.toml` | nix dependency | VERIFIED | `nix = { version = "0.29", default-features = false, features = ["signal", "process"] }` |
| `src/backend/mod.rs` | Backend enum, pub use exports | VERIFIED | Backend::Http/Stdio with send(), exports StdioBackend, run_supervisor, discover_stdio_tools |
| `src/gateway.rs` | Dispatch routes to Backend (not just HttpBackend) | VERIFIED | `backends: &HashMap<String, Backend>` in run_dispatch and handle_tools_call |
| `src/main.rs` | Stdio supervisor spawning and shutdown | VERIFIED | Filters stdio configs, spawns supervisors, awaits discovery, shutdown with cancel + wait |
| `tests/stdio_integration.rs` | Integration tests (>50 lines) | VERIFIED | 229 lines, 3 tests proving end-to-end stdio routing |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| stdio.rs | tokio::process::Command | process_group(0) spawn | WIRED | Line 40: `.process_group(0)` |
| stdio.rs | nix::sys::signal::killpg | process group termination | WIRED | Line 288: `signal::killpg(pgid, Signal::SIGTERM)` |
| stdio.rs | pending HashMap | oneshot channels for correlation | WIRED | send() inserts, stdout_reader removes and sends response |
| supervisor | StdioBackend::spawn | respawn on crash | WIRED | `StdioBackend::spawn(&config)` called in supervisor loop after backoff |
| supervisor | CancellationToken | graceful shutdown | WIRED | `cancel.is_cancelled()` pre-spawn, `cancel.cancelled()` in select during monitor and backoff |
| supervisor | discover_stdio_tools | MCP handshake after spawn | WIRED | `discover_stdio_tools(&backend).await` called after each spawn |
| gateway.rs | backend/stdio.rs | StdioBackend::send() via Backend enum | WIRED | `Backend::Stdio(s) => s.send(json_rpc_body).await` |
| main.rs | backend/stdio.rs | spawn supervisors from config | WIRED | `run_supervisor(cfg, cancel_clone, tools_tx)` in tokio::spawn |
| gateway.rs | Backend enum | unified dispatch | WIRED | `Backend::Http` and `Backend::Stdio` in mod.rs, used throughout gateway |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| STDIO-01 | 08-01 | Gateway spawns stdio backend processes from config | SATISFIED | StdioBackend::spawn() takes BackendConfig with command, args, env; main.rs spawns from config |
| STDIO-02 | 08-02 | Gateway manages stdio backend lifecycle (health monitoring, crash detection) | SATISFIED | Supervisor detects crashes via stdout EOF, monitors via tokio::select on stdout_handle |
| STDIO-03 | 08-02 | Gateway restarts crashed stdio backends with exponential backoff | SATISFIED | backoff_delay() with 1s base, 60s cap, 0-50% jitter; restart_count tracks and resets after 60s healthy |
| STDIO-04 | 08-01 | Gateway multiplexes concurrent JSON-RPC via request ID correlation | SATISFIED | pending HashMap<u64, oneshot::Sender<String>> with insert in send(), route in stdout_reader |
| STDIO-05 | 08-01 | Gateway cleanly terminates stdio backends on shutdown (process group kill) | SATISFIED | kill_process_group() uses nix::killpg with SIGTERM; main.rs shutdown calls cancel.cancel() triggering supervisor kill |
| ROUTE-02 | 08-03 | Gateway routes tools/call to correct stdio backend based on tool name | SATISFIED | Backend enum unifies dispatch; catalog.route() resolves tool to backend name; backends_map lookup gets Backend::Stdio |

No orphaned requirements found for Phase 8.

Note: REQUIREMENTS.md tracking table has STDIO-01, STDIO-04, STDIO-05 listed as "Pending" but the checkbox list at the top has them checked "[x]". This is a documentation inconsistency only -- the implementation fully satisfies all six requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns found |

### Human Verification Required

### 1. Live stdio backend startup with real MCP servers

**Test:** Configure sentinel-gateway.toml with a real stdio backend (e.g., context7 via `npx @upstash/context7-mcp`) and start the gateway.
**Expected:** Backend spawns, MCP handshake succeeds, tools appear in tools/list, tools/call returns real results.
**Why human:** Integration tests use a mock Python MCP server; real npx-based servers have startup latency, npm dependencies, and real protocol behavior that can differ.

### 2. Crash recovery with real stdio backends

**Test:** Start gateway with a stdio backend, then kill the child process externally (`kill -9 <pid>`). Observe logs.
**Expected:** Supervisor detects crash, logs warning, applies backoff, respawns, re-discovers tools, backend becomes available again.
**Why human:** Unit tests use `true` (instant exit) which never completes MCP handshake; real crash-and-recover flow needs a working MCP server.

### 3. Shutdown terminates all child processes

**Test:** Start gateway with stdio backends, send SIGTERM to gateway process, check that no orphan child processes remain.
**Expected:** All child processes terminated, no zombie/orphan processes, gateway exits cleanly.
**Why human:** Process group kill behavior depends on OS process management; programmatic verification requires running the full binary.

### Gaps Summary

No gaps found. All 14 observable truths verified, all 7 artifacts substantive and wired, all 9 key links confirmed, all 6 requirements satisfied. 6 unit tests and 3 integration tests pass. No anti-patterns detected. Clean compilation with no warnings.

---

_Verified: 2026-02-22T07:15:00Z_
_Verifier: Claude (gsd-verifier)_
