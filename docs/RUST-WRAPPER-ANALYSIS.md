# Rust Wrapper Analysis

Technical deep-dive on the existing Rust stdio-to-HTTP bridge at `/home/lwb3/mcp-context-forge/tools_rust/wrapper/`.

## Overview

The MCP stdio wrapper is a ~890-line Rust application that bridges Claude Code's stdio transport to the ContextForge HTTP gateway. It's the only Rust code in the current stack and represents a starting point for understanding MCP protocol handling in Rust.

**Version:** 1.0.0-RC-1
**Location:** `/home/lwb3/mcp-context-forge/tools_rust/wrapper/`

## Architecture

```
STDIN (JSON-RPC lines)
    |
    v
stdio_reader (async task)
    |  flume channel (unbounded)
    v
MCP Workers Pool (N concurrent, default 10)
    |  Each worker:
    |  1. Parse JSON-RPC request
    |  2. POST to gateway via HTTP/SSE
    |  3. Stream response bytes
    |  4. Extract lines, strip SSE "data:" prefix
    |
    v  flume channel (unbounded)
stdio_writer (async task, batched flush)
    |
    v
STDOUT (JSON-RPC lines)
```

## Module Breakdown (25 source files)

### Core (4 modules, ~100 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `main.rs` | 12 | Entry point, mimalloc global allocator, tokio main |
| `lib.rs` | 27 | Module re-exports |
| `main_init.rs` | 19 | Parse CLI args, initialize logger |
| `main_loop.rs` | 53 | Orchestrate reader/workers/writer, create HTTP client |

### Configuration (2 modules, ~90 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `config.rs` | 73 | `Config` struct — CLI args via clap + env var bindings |
| `config_from_cli.rs` | 17 | `Config::from_cli()` parser |

### I/O & Transport (4 modules, ~130 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `stdio_reader.rs` | 25 | Read lines from stdin -> flume channel |
| `stdio_writer.rs` | 23 | Batched write from channel -> stdout |
| `stdio_process.rs` | 37 | Append newlines, flush batches |
| `http_client.rs` | 42 | Build reqwest::Client with TLS, timeout, pool config |

### MCP Gateway Communication (8 modules, ~280 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `streamer.rs` | 14 | `McpStreamClient` struct definition |
| `streamer_new.rs` | 35 | Initialize headers (ACCEPT, CONTENT_TYPE, AUTH) |
| `streamer_send.rs` | 30 | Build and send HTTP POST with session tracking |
| `streamer_post.rs` | 58 | Stream response, detect SSE, extract session ID |
| `streamer_session.rs` | 39 | Lock-free session ID (ArcSwap) |
| `streamer_id.rs` | 33 | Extract session ID from response headers |
| `streamer_auth.rs` | 8 | Check if auth header is configured |
| `streamer_lines.rs` | 26 | Split response buffer on LF/CRLF |
| `streamer_error.rs` | 65 | Build JSON-RPC error responses |

### Workers (3 modules, ~130 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `mcp_workers.rs` | 69 | Spawn N workers, loop on channel, POST + write |
| `mcp_workers_write.rs` | 53 | Strip SSE "data:" prefix, trim whitespace |
| `post_result.rs` | 12 | `PostResult { out: Vec<Bytes>, sse: bool }` |

### JSON-RPC ID Parsing (2 modules, ~90 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `json_rpc_id.rs` | 15 | Full deserialization path (slow, fallback) |
| `json_rpc_id_fast.rs` | 76 | Streaming actson parser — extract ID at depth 1 only |

### Logging (1 module, 69 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `logger.rs` | 69 | tracing subscriber with file/stderr output, env-filter |

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1.49.0 | Async runtime (full features) |
| `reqwest` | 0.13.1 | HTTP client (rustls, json, stream, http2) |
| `serde` + `serde_json` | 1.0.228 / 1.0.149 | JSON serialization |
| `tracing` + subscribers | 0.1.44 / 0.3.22 | Structured logging |
| `clap` | 4.5.54 | CLI argument parsing (derive + env) |
| `jsonrpc-core` | 18.0.0 | JSON-RPC 2.0 types |
| `rustls` | 0.23 | TLS (aws_lc_rs backend) |
| `flume` | 0.12.0 | High-performance MPMC channels |
| `futures` | 0.3.31 | Stream utilities |
| `actson` | 2.1.0 | Streaming JSON parser (fast ID extraction) |
| `bytes` | 1.11.1 | Zero-copy byte buffers |
| `arc-swap` | 1.8.1 | Lock-free atomic pointer (session ID) |
| `mimalloc` | 0.1.48 | High-performance allocator |
| `dotenvy` | 0.15.7 | .env file loading |

## CLI Arguments

| Flag | Default | Env Var | Purpose |
|------|---------|---------|---------|
| `--url` | (required) | `MCP_SERVER_URL` | Gateway endpoint URL |
| `--auth` | None | `MCP_AUTH` | Authorization header (`Bearer <token>`) |
| `--concurrency` | 10 | `CONCURRENCY` | Max concurrent workers |
| `--log-level` | off | `LOG_LEVEL` | trace/debug/info/warn/error/off |
| `--log-file` | None | `MCP_LOG_FILE` | Log to file instead of stderr |
| `--timeout` | 60s | `MCP_TOOL_CALL_TIMEOUT` | HTTP request timeout |
| `--tls-cert` | None | `TLS_CERT` | Custom CA cert (PEM) |
| `--content-type` | application/json | `MCP_CONTENT_TYPE` | Content-Type header |
| `--http-pool-per-worker` | false | `HTTP_POOL_PER_WORKER` | Separate client pool per worker |
| `--http-pool-size` | None | `HTTP_POOL_SIZE` | Max idle connections per host |
| `--http2` | false | `HTTP2` | Enable HTTP/2 |
| `--insecure` | false | `INSECURE` | Skip TLS verification |

## Performance Optimizations

1. **mimalloc** global allocator for faster allocation
2. **LTO (fat)** + single codegen unit for maximum runtime speed
3. **Zero-copy bytes** — `bytes::Bytes` for reference-counted splits
4. **Streaming HTTP** — no full response buffering, chunks processed as they arrive
5. **Fast JSON ID parser** — actson streaming parser avoids full deserialization
6. **Lock-free session ID** — ArcSwap instead of Mutex
7. **Batched stdout writes** — accumulate + single flush
8. **TCP_NODELAY** — disabled Nagle's algorithm

## Key Design Patterns

### Lock-Free Session ID
Uses `arc_swap::ArcSwap<Option<String>>` — no mutexes. All workers share one session ID with compare-and-swap updates.

### Unbounded Channels
Reader -> Workers and Workers -> Writer use flume unbounded channels. No backpressure — if the gateway is slow, memory grows unbounded.

### SSE Detection & Stripping
Detects `Content-Type: text/event-stream`, strips `"data: "` prefix from each line. This handles the ContextForge SSE transport.

### Per-Worker vs Shared HTTP Client
`--http-pool-per-worker` creates separate connection pools per worker (higher memory, potentially better concurrency). Default shares one pool across all workers.

## What's Reusable for the Rust Gateway

| Component | Reusable? | Notes |
|-----------|-----------|-------|
| HTTP client setup | Yes | reqwest + rustls config, connection pooling |
| JSON-RPC ID parsing | Yes | Fast path extraction for request correlation |
| stdio reader/writer | Yes | Clean async stdin/stdout handling |
| Session management | Partial | ArcSwap pattern good, but gateway needs multi-session |
| Config/CLI structure | Yes | clap + env var pattern |
| Logging setup | Yes | tracing + file output |
| Worker pool pattern | Partial | Gateway needs request routing, not just forwarding |
| SSE handling | Yes | Needed for Streamable HTTP transport |

## What's Missing for a Gateway

The wrapper only forwards requests. A gateway needs:

- **Routing logic** — match tool names to backend servers
- **Authentication** — validate incoming tokens, not just forward them
- **RBAC** — per-tool, per-role authorization
- **Rate limiting** — per-client, per-tool counters
- **Audit logging** — structured records of every tool call
- **Health checks** — monitor backend availability
- **Tool discovery** — aggregate tool schemas from multiple backends
- **Policy engine** — allow/deny rules beyond simple RBAC
- **Kill switch** — emergency disable for individual tools or backends
- **Configuration persistence** — backend registry, not just CLI flags

## Build & Test

```bash
cd /home/lwb3/mcp-context-forge/tools_rust/wrapper/
make build-release    # Optimized binary with LTO
make test             # Run test suite (~350 lines, 15 test modules)
make pedantic         # clippy pedantic lints
make coverage         # llvm-cov code coverage
```

**Note:** Rust builds require `dangerouslyDisableSandbox: true` due to bwrap loopback permission errors.
