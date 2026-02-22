# Phase 8: stdio Backend Management - Research

**Researched:** 2026-02-22
**Domain:** Rust async process management, MCP stdio transport, JSON-RPC multiplexing
**Confidence:** HIGH

## Summary

Phase 8 implements the gateway's unique differentiator: managing stdio-based MCP servers as child processes. The gateway must spawn processes from config, communicate via newline-delimited JSON-RPC over stdin/stdout, multiplex concurrent requests using request ID correlation, detect crashes and restart with backoff, and cleanly terminate entire process groups on shutdown.

The technical approach is well-defined: Tokio's `tokio::process::Command` handles async subprocess spawning with piped stdin/stdout. The `process_group(0)` method (Unix-only, available in tokio) creates a new process group per child so the gateway can `killpg` the entire tree (critical for npx/node processes that spawn grandchildren). Request multiplexing uses the existing `IdRemapper` pattern -- write requests to stdin with gateway-assigned IDs, read responses from stdout, correlate by ID, and route back to the original caller.

**Primary recommendation:** Create a `StdioBackend` struct parallel to `HttpBackend` that owns a spawned child process, exposes an async `send()` method matching the HTTP backend interface, and manages lifecycle internally. The gateway dispatch should use a `Backend` enum (or trait object) so `handle_tools_call` routes to either HTTP or stdio backends transparently.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| STDIO-01 | Gateway spawns stdio backend processes from config (command + args + env vars) | `tokio::process::Command` with `.stdin(Stdio::piped()).stdout(Stdio::piped())`, config already has `command`, `args`, `env` fields in `BackendConfig` |
| STDIO-02 | Gateway manages stdio backend lifecycle (health monitoring, crash detection) | Monitor `child.wait()` future -- when it completes, the process has exited. Health check via JSON-RPC `ping` over stdin/stdout |
| STDIO-03 | Gateway restarts crashed stdio backends with exponential backoff | Spawn supervisor task per backend that watches child exit, applies backoff (base 1s, max 60s, jitter), respects `max_restarts` from config |
| STDIO-04 | Gateway multiplexes concurrent JSON-RPC requests over single stdin/stdout using request ID correlation | Use existing `IdRemapper` pattern. Pending requests stored in `Arc<Mutex<HashMap<u64, oneshot::Sender>>>`. Stdout reader task parses responses, looks up ID, sends via oneshot |
| STDIO-05 | Gateway cleanly terminates stdio backends on shutdown (process group kill) | `.process_group(0)` on spawn, then `nix::sys::signal::killpg(pid, SIGTERM)` on shutdown. Fallback to SIGKILL after timeout |
| ROUTE-02 | Gateway routes tools/call requests to correct stdio backend based on tool name | Extend `handle_tools_call` to look up stdio backends in addition to HTTP backends. Catalog routing already works -- just need the backend map to include stdio entries |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.47 | Async process spawning, channels, timers | Already in project; `tokio::process::Command` provides async child process management |
| nix | 0.29 | `killpg()` for process group termination | Standard Rust crate for POSIX APIs; `tokio::process::Child::kill()` only kills direct child, not process group |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio (oneshot) | 1.47 | Per-request response channels for multiplexing | Already available; `tokio::sync::oneshot` for request-response correlation |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| nix for killpg | unsafe libc::killpg directly | nix provides safe wrapper, worth the 1 dependency |
| oneshot channels for mux | DashMap + Notify | oneshot is simpler, zero contention for single-consumer pattern |

**Installation:**
```bash
cargo add nix --features signal,process
```

## Architecture Patterns

### Recommended Module Structure
```
src/backend/
  mod.rs          # Add StdioBackend export
  http.rs         # Existing (unchanged)
  stdio.rs        # NEW: StdioBackend struct + multiplexer + supervisor
  error.rs        # Add stdio-specific error variants
  retry.rs        # Existing (unchanged)
  sse.rs          # Existing (unchanged)
```

### Pattern 1: StdioBackend with Internal Multiplexer

**What:** A `StdioBackend` struct that owns the child process lifecycle and provides an async `send(&self, json_rpc_body: &str) -> Result<String, BackendError>` method matching `HttpBackend::send()`.

**When to use:** For all stdio backend communication.

**Architecture:**
```
StdioBackend
  |-- stdin_tx: mpsc::Sender<String>          (write requests to child stdin)
  |-- pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>  (response routing)
  |-- supervisor_handle: JoinHandle<()>        (crash detection + restart)
```

**Example:**
```rust
// Source: Architectural pattern derived from tokio::process docs
pub struct StdioBackend {
    stdin_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    name: String,
}

impl StdioBackend {
    pub async fn send(&self, json_rpc_body: &str) -> Result<String, BackendError> {
        // Parse the request to extract the gateway-assigned ID
        let parsed: serde_json::Value = serde_json::from_str(json_rpc_body)
            .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
        let id = parsed.get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| BackendError::InvalidResponse("missing id".into()))?;

        // Create oneshot channel for this request's response
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            pending.insert(id, resp_tx);
        }

        // Write to child stdin
        self.stdin_tx.send(json_rpc_body.to_string()).await
            .map_err(|_| BackendError::InvalidResponse("stdin closed".into()))?;

        // Wait for response (with timeout)
        match tokio::time::timeout(Duration::from_secs(60), resp_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(BackendError::InvalidResponse("response channel dropped".into())),
            Err(_) => {
                // Clean up pending entry on timeout
                self.pending.lock().unwrap().remove(&id);
                Err(BackendError::InvalidResponse("request timed out".into()))
            }
        }
    }
}
```

### Pattern 2: Stdout Reader Task

**What:** A background task that reads newline-delimited JSON from the child's stdout, parses the response ID, and routes to the correct pending oneshot channel.

**Example:**
```rust
// Spawned per stdio backend
async fn stdout_reader(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF -- process exited
            Ok(_) => {
                let trimmed = line.trim_end().to_string();
                if trimmed.is_empty() { continue; }
                // Parse just enough to get the ID
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&trimmed) {
                    if let Some(id) = parsed.get("id").and_then(|v| v.as_u64()) {
                        let sender = pending.lock().unwrap().remove(&id);
                        if let Some(tx) = sender {
                            let _ = tx.send(trimmed);
                        }
                    }
                    // Notifications (no id) can be logged/ignored
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "stdout read error");
                break;
            }
        }
    }
}
```

### Pattern 3: Supervisor Task with Exponential Backoff

**What:** A per-backend task that monitors child process exit, handles restart with backoff, and respects `max_restarts` limit.

**Example:**
```rust
async fn supervisor(
    config: BackendConfig,
    stdin_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    cancel: CancellationToken,
) {
    let mut restart_count: u32 = 0;
    let mut backoff_secs: u64 = 1;

    loop {
        let mut child = spawn_child(&config);
        let pid = child.id().expect("child has pid");
        tracing::info!(backend = %config.name, pid, "stdio backend started");

        // Take stdin/stdout handles
        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();

        // Spawn stdin writer and stdout reader tasks
        // ... (wire up channels)

        // Wait for child exit or cancellation
        tokio::select! {
            status = child.wait() => {
                tracing::warn!(backend = %config.name, ?status, "stdio backend exited");
                // Fail all pending requests
                drain_pending(&pending);

                if !config.restart_on_exit || restart_count >= config.max_restarts {
                    tracing::error!(backend = %config.name, "max restarts reached");
                    break;
                }
                restart_count += 1;
                let jitter = rand::random::<f64>() * 0.5;
                let delay = Duration::from_secs_f64(backoff_secs as f64 * (1.0 + jitter));
                tracing::info!(backend = %config.name, delay_secs = ?delay, "restarting after backoff");
                tokio::time::sleep(delay).await;
                backoff_secs = (backoff_secs * 2).min(60);
            }
            _ = cancel.cancelled() => {
                tracing::info!(backend = %config.name, "shutting down stdio backend");
                kill_process_group(pid);
                let _ = child.wait().await;
                break;
            }
        }
    }
}
```

### Pattern 4: Process Group Spawn and Kill

**What:** Spawn child in its own process group so we can kill the entire tree.

**Example:**
```rust
use std::process::Stdio;
use tokio::process::Command;

fn spawn_child(config: &BackendConfig) -> tokio::process::Child {
    let mut cmd = Command::new(config.command.as_deref().unwrap());
    cmd.args(&config.args)
       .envs(&config.env)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped())
       .stderr(Stdio::piped())
       .kill_on_drop(false)  // We handle kill ourselves
       .process_group(0);    // New process group (PGID = child PID)
    cmd.spawn().expect("failed to spawn stdio backend")
}

fn kill_process_group(pid: u32) {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;
    let pgid = Pid::from_raw(pid as i32);
    if let Err(e) = killpg(pgid, Signal::SIGTERM) {
        tracing::warn!(pid, error = %e, "killpg SIGTERM failed");
    }
}
```

### Pattern 5: MCP Handshake for Tool Discovery

**What:** Before routing traffic, perform `initialize` -> `notifications/initialized` -> `tools/list` over stdio, same as HTTP backends do.

**Key difference from HTTP:** Responses come back on stdout asynchronously. The multiplexer handles this naturally -- send initialize request with a known ID, wait for the response via the pending map.

### Anti-Patterns to Avoid
- **Using `kill_on_drop(true)` alone:** Only kills the direct child PID, not the process group. Node processes spawn grandchildren that become orphans.
- **Using `child.kill()` for shutdown:** Same problem -- only kills direct child. Must use `killpg` on the process group.
- **Using npx at runtime:** Creates extra wrapper processes, slower startup, network dependency. Pre-install packages globally with `npm install -g`.
- **Blocking stdin writes:** If the child's stdout buffer fills and the reader isn't consuming, stdin writes will deadlock. Always run reader and writer as separate tasks.
- **Shared stdout reader for multiple backends:** Each stdio backend has its own process with its own stdout. Never mix streams.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Process group kill | Manual libc calls | `nix::sys::signal::killpg()` | Safe wrapper, handles ESRCH (process already dead) |
| Async line reading | Manual byte-level parsing | `tokio::io::BufReader::read_line()` | Handles partial reads, UTF-8, buffering correctly |
| Exponential backoff | Custom math | Reuse pattern from `backend/retry.rs` | Already proven in project, consistent behavior |
| Process spawning | std::process | `tokio::process::Command` | Async-aware, integrates with tokio runtime |

## Common Pitfalls

### Pitfall 1: npx Creates Process Groups
**What goes wrong:** `npx @upstash/context7-mcp` spawns node via npx wrapper. Killing the npx process leaves node running.
**Why it happens:** npx is a shell wrapper that exec's node. On some systems it creates intermediate processes.
**How to avoid:** Config already uses `command = "node"` with direct path to installed JS file. Never use npx in production. Pre-install with `npm install -g`.
**Warning signs:** Orphan node processes after gateway shutdown. `ps aux | grep node` shows processes with PPID=1.

### Pitfall 2: Stdin/Stdout Deadlock
**What goes wrong:** Gateway writes to stdin but never reads stdout. Child's stdout buffer fills (default 64KB pipe buffer on Linux), child blocks on write, stops reading stdin, gateway blocks on stdin write.
**Why it happens:** Both sides waiting for the other to consume.
**How to avoid:** Always spawn separate reader and writer tasks. Never block the reader.
**Warning signs:** Gateway hangs after sending a request. No response within timeout.

### Pitfall 3: Response Without Matching Request
**What goes wrong:** Child sends a JSON-RPC notification (no `id` field) or a response with an ID that was already cleaned up (timeout).
**Why it happens:** MCP servers can send notifications. Or responses arrive after timeout cleanup.
**How to avoid:** Stdout reader must handle: (a) messages with no `id` (log and ignore), (b) messages with unknown `id` (log and drop). Never panic on unmatched IDs.
**Warning signs:** Log messages about unmatched response IDs.

### Pitfall 4: Zombie Processes
**What goes wrong:** Child exits but gateway never calls `wait()`. Process stays in zombie state.
**Why it happens:** Forgot to await the child's exit status.
**How to avoid:** Supervisor task always calls `child.wait()` after detecting exit or sending kill signal.
**Warning signs:** `ps aux` shows processes in Z (zombie) state.

### Pitfall 5: Race Between Shutdown and Restart
**What goes wrong:** Child crashes, supervisor starts restart backoff, then gateway receives SIGTERM. Supervisor spawns new child during shutdown.
**Why it happens:** CancellationToken not checked between crash detection and respawn.
**How to avoid:** Check `cancel.is_cancelled()` before respawning. Use `tokio::select!` on backoff sleep vs cancellation.
**Warning signs:** New child processes appearing during shutdown sequence.

### Pitfall 6: Environment Variable Secrets
**What goes wrong:** Env vars with API keys (e.g., firecrawl's `FIRECRAWL_API_KEY`) not passed to child process.
**Why it happens:** `Command::envs()` replaces inherited env. Need to either inherit parent env and add extras, or explicitly include required vars.
**How to avoid:** Use `.env()` for each additional var (preserves parent env inheritance), OR use `.envs()` with a merged map. The config `env` field is for extras on top of inherited env.
**Warning signs:** Child process fails with "missing API key" errors.

## Code Examples

### Spawning a stdio backend with process group
```rust
// Source: tokio::process docs + nix docs
use std::os::unix::process::CommandExt as StdCommandExt;
use std::process::Stdio;
use tokio::process::Command;

let mut cmd = Command::new("node");
cmd.args(&["/usr/local/lib/node_modules/@upstash/context7-mcp/dist/index.js"])
   .stdin(Stdio::piped())
   .stdout(Stdio::piped())
   .stderr(Stdio::piped())
   .process_group(0)       // PGID = child PID
   .kill_on_drop(false);   // We manage lifecycle

// Add extra env vars (parent env is inherited by default)
cmd.env("SOME_KEY", "value");

let child = cmd.spawn()?;
```

### Killing an entire process group
```rust
// Source: nix docs - killpg
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;

fn terminate_process_group(pid: u32) {
    let pgid = Pid::from_raw(pid as i32);
    match killpg(pgid, Signal::SIGTERM) {
        Ok(()) => tracing::info!(pid, "Sent SIGTERM to process group"),
        Err(nix::errno::Errno::ESRCH) => {
            tracing::debug!(pid, "Process group already exited");
        }
        Err(e) => tracing::error!(pid, error = %e, "killpg failed"),
    }
}
```

### Gateway dispatch integration (conceptual)
```rust
// In gateway.rs handle_tools_call, extend backend lookup:
enum BackendRef<'a> {
    Http(&'a HttpBackend),
    Stdio(&'a StdioBackend),
}

// Or simpler: trait Backend { async fn send(&self, body: &str) -> Result<String, BackendError>; }
// Both HttpBackend and StdioBackend implement it.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| npx for MCP servers | Global npm install + direct node invocation | MCP best practice 2025 | Faster startup, no network dependency, no wrapper processes |
| Kill child PID only | process_group(0) + killpg | Always been the correct approach | Prevents orphan processes from node/npx |
| Separate backend maps | Unified Backend trait/enum | This phase introduces it | Gateway dispatch doesn't care about transport type |

## Open Questions

1. **Stderr handling for stdio backends**
   - What we know: MCP spec says servers MAY write to stderr for logging. Clients MAY capture, forward, or ignore.
   - Recommendation: Pipe stderr and log it at DEBUG level with backend name prefix. Simple and useful for debugging.

2. **Tool discovery retry for stdio backends**
   - What we know: Stdio backends may take time to initialize (node startup). First `tools/list` might fail.
   - Recommendation: Add a small delay (1-2s) after spawn before running MCP handshake, or retry the handshake with backoff.

3. **Backend abstraction: trait vs enum**
   - What we know: `handle_tools_call` currently takes `HashMap<String, HttpBackend>`. Need to add stdio.
   - Recommendation: Use an enum `Backend { Http(HttpBackend), Stdio(StdioBackend) }` with a `send()` method. Simpler than trait objects, no dynamic dispatch overhead, matches project's preference for concrete types.

## Sources

### Primary (HIGH confidence)
- [MCP Specification - Transports (2025-06-18)](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) - stdio transport framing rules, newline delimiters, message format
- [tokio::process::Command docs](https://docs.rs/tokio/latest/tokio/process/struct.Command.html) - process_group(0), kill_on_drop, stdin/stdout piping
- [tokio::process::Child docs](https://docs.rs/tokio/latest/tokio/process/struct.Child.html) - wait(), kill(), stdin/stdout handles
- [nix::sys::signal::killpg docs](https://docs.rs/nix/latest/nix/sys/signal/fn.killpg.html) - process group signal delivery

### Secondary (MEDIUM confidence)
- [rustix::process::kill_process_group](https://docs.rs/rustix/latest/rustix/process/fn.kill_process_group.html) - alternative to nix (not recommended, nix is more established)

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - tokio::process is the standard, nix is well-established for POSIX
- Architecture: HIGH - patterns derived from existing codebase (HttpBackend, IdRemapper) and official tokio docs
- Pitfalls: HIGH - npx gotcha documented in STATE.md, deadlock/zombie patterns are well-known Unix programming issues

**Research date:** 2026-02-22
**Valid until:** 2026-04-22 (stable domain, tokio/nix APIs unlikely to change)
