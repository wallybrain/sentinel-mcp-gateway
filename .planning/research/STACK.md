# Technology Stack

**Project:** Sentinel Gateway (Rust MCP Gateway)
**Researched:** 2026-02-22

## Recommended Stack

### Async Runtime

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| tokio | 1.47.x (LTS) | Async runtime | Industry standard. LTS until Sept 2026. All ecosystem crates (axum, reqwest, sqlx) build on tokio. Use LTS over bleeding-edge 1.49 for stability. | HIGH |
| tokio-util | 0.7.x | Codec, stream utilities | Needed for stdio line framing and SSE stream processing. | HIGH |

### MCP Protocol

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| rmcp | 0.16.x | MCP protocol types + transport | Official Rust SDK from modelcontextprotocol org. Implements MCP spec 2025-11-25 with Streamable HTTP, stdio, and task lifecycle. Avoids hand-rolling JSON-RPC 2.0 + MCP capability negotiation. | MEDIUM |
| serde + serde_json | 1.x / 1.x | JSON serialization | Universal Rust serialization. rmcp and every other crate depend on it. | HIGH |

**Why rmcp over hand-rolling JSON-RPC:**
- MCP is more than JSON-RPC -- it includes capability negotiation, tool schema types, session management, SSE framing, and the 2025-11-25 protocol spec.
- rmcp provides type-safe request/response structures matching the spec, including `ToolInfo`, `CallToolResult`, `ServerCapabilities`, etc.
- The crate is at 0.16 with rapid iteration (22 releases since March 2025). This means: use it for types and protocol, but own your transport and routing layer.

**Risk:** rmcp is pre-1.0 with 35% doc coverage. Pin exact versions and wrap its types behind internal traits so you can swap if needed. The gateway is a custom router, not a standard MCP server -- you use rmcp for protocol types, not its server runtime.

### HTTP Server (Upstream-facing)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| axum | 0.8.x | HTTP server framework | Tokio-native, Tower middleware ecosystem, best DX in Rust web. v0.8 released Jan 2025 with stable API. Extractors for JSON, headers, state. SSE support built in. | HIGH |
| tower | 0.5.x | Middleware framework | Axum is built on Tower. Use Tower layers for auth, rate limiting, logging, timeout -- composable middleware stack. | HIGH |
| tower-http | 0.6.x | HTTP-specific middleware | CORS, compression, tracing, timeout layers. Production-tested with axum. | HIGH |

**Why axum over alternatives:**
- **Not Actix Web:** Actix has its own runtime and actor model. Adds complexity without benefit when already on tokio. Axum's Tower integration gives cleaner middleware composition.
- **Not Rocket:** Rocket has a more opinionated design, less flexible for custom protocol handling (JSON-RPC over HTTP, SSE).
- **Not Warp:** Warp's filter-based API produces harder-to-read code and worse error messages. Axum supersedes it in the tokio ecosystem.

### HTTP Client (Downstream to backends)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| reqwest | 0.13.x | HTTP client to backends | Already proven in the existing wrapper. Connection pooling, streaming responses, rustls TLS, HTTP/2. | HIGH |
| rustls | 0.23.x | TLS (no OpenSSL) | Pure-Rust TLS. No system OpenSSL dependency = simpler Docker builds with scratch/distroless base. aws_lc_rs backend for FIPS-grade crypto. | HIGH |

### Authentication

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| jsonwebtoken | 10.x | JWT encode/decode/validate | De facto Rust JWT library. HS256 for v1 (matches current ContextForge). Supports clock skew leeway, custom claims, algorithm allowlisting. Use aws_lc_rs backend (same as rustls). | HIGH |

**JWT flow:** Client sends `Authorization: Bearer <token>`. Axum extractor validates signature + expiry + claims. Claims include role for RBAC lookup. No external auth service needed for v1.

### Database

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| sqlx | 0.8.x | Async Postgres driver | Compile-time checked queries (catches SQL errors at build time). Pure async, no ORM overhead. Migrations built in. Connection pooling via sqlx::PgPool. | HIGH |

**Why sqlx over alternatives:**
- **Not Diesel:** Diesel requires a DSL and has a steeper learning curve. sqlx lets you write raw SQL with compile-time checking -- better for a gateway with simple audit/config schemas.
- **Not SeaORM:** ORM abstraction is unnecessary overhead for audit log inserts and config reads. sqlx is closer to the metal.
- **Schema needs:** Audit logs (who/what/when/args/status), rate limit state (counters), backend config, RBAC rules. Simple relational schema, not ORM territory.

### Rate Limiting

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| governor | 0.10.x | Token bucket rate limiting | GCRA algorithm (functionally equivalent to leaky bucket). Keyed rate limiters for per-client-per-tool limits. Memory-efficient (64 bits per state). Thread-safe with CAS operations. | HIGH |
| tower-governor | 0.6.x | Tower/Axum integration | Wraps governor as a Tower layer. Drops into the axum middleware stack. | MEDIUM |

**Note:** tower-governor may not support per-tool keying out of the box. Likely need a custom Tower layer wrapping governor's `RateLimiter<K>` with `(client_id, tool_name)` as the key type. governor itself supports this natively via `RateLimiter::keyed()`.

### Configuration

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| toml | 0.8.x | TOML parsing | Standard Rust config format. Familiar from Cargo.toml. Fast, well-maintained (~416M downloads). | HIGH |
| serde | 1.x | Config deserialization | Deserialize TOML directly into typed Rust structs. Compile-time validation of config shape. | HIGH |
| dotenvy | 0.15.x | .env file loading | Already used in wrapper. Loads secrets from .env without committing them. | HIGH |

**Why TOML + serde directly, not the `config` crate:**
- The `config` crate adds layered merging and multiple format support that adds complexity without value. TOML files + env var overrides via clap/dotenvy cover all needs.
- Config struct with `#[derive(Deserialize)]` + `toml::from_str()` is 5 lines, fully typed, and compile-time checked.
- Hot reload: `notify` crate (0.7.x) watches config file, re-parse on change, swap via `ArcSwap`.

### CLI

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| clap | 4.5.x | CLI argument parsing | Derive macros for zero-boilerplate CLI. Env var bindings built in. Already used in wrapper. | HIGH |

### Observability

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| tracing | 0.1.x | Structured logging/tracing | Industry standard Rust observability. Structured fields, span context, async-aware. Already used in wrapper. | HIGH |
| tracing-subscriber | 0.3.x | Log output formatting | EnvFilter for runtime log level control. JSON formatter for machine-readable logs. File + stderr output. | HIGH |
| tracing-appender | 0.2.x | Non-blocking log writer | Async file appender so logging doesn't block the event loop. | HIGH |

**Audit logging strategy:** tracing handles operational logs (debug, errors, performance). Audit logs go to Postgres via sqlx (structured records with query capability). These are separate concerns -- don't conflate them.

### Process Management (stdio backends)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| tokio::process | (part of tokio) | Spawn child processes | Async child process management for stdio-based MCP backends (context7, firecrawl, exa, etc.). Stdin/stdout/stderr pipes. | HIGH |
| flume | 0.12.x | MPMC channels | High-performance channels between stdio reader/writer tasks and request router. Already proven in wrapper. | HIGH |

### Error Handling

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| thiserror | 2.x | Error type derivation | Derive `Error` impls with `#[error("...")]` format strings. Clean error enums for gateway-specific errors. | HIGH |
| anyhow | 1.x | Application error context | For top-level error chains in main/startup. NOT in library code -- use thiserror for typed errors in the core. | HIGH |

### Performance

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| mimalloc | 0.1.x | Global allocator | Faster than system allocator for concurrent workloads. Already used in wrapper. Drop-in replacement. | HIGH |
| bytes | 1.x | Zero-copy byte buffers | Reference-counted byte slices. Avoid copying HTTP response bodies. Already used in wrapper. | HIGH |
| arc-swap | 1.x | Lock-free atomic pointer | Hot-swap config and rate limit state without locks. Already used in wrapper for session ID. | HIGH |

### Testing

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| tokio::test | (part of tokio) | Async test runtime | `#[tokio::test]` for async unit tests. | HIGH |
| axum::test | (part of axum) | HTTP handler testing | `TestClient` for integration tests without starting a real server. | HIGH |
| wiremock | 0.6.x | HTTP mock server | Mock backend MCP servers in tests. Verify request shapes, simulate errors, test retries. | MEDIUM |
| testcontainers | 0.23.x | Postgres in tests | Spin up real Postgres in Docker for integration tests. Tests run against real DB, not mocks. | MEDIUM |
| cargo-llvm-cov | (CLI tool) | Code coverage | Already used in wrapper build system. | HIGH |

### Docker / Deployment

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| Docker multi-stage | -- | Build container | Stage 1: `rust:1.83-slim` for build. Stage 2: `debian:bookworm-slim` for runtime (~80 MB). Could go distroless (~30 MB) but bookworm-slim is easier to debug. | HIGH |
| docker-compose | -- | Service orchestration | Gateway + Postgres. Matches current VPS pattern. `restart: unless-stopped`. Health checks. | HIGH |

**Docker build strategy:**
```dockerfile
# Stage 1: Build
FROM rust:1.83-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/sentinel-gateway /usr/local/bin/
ENTRYPOINT ["sentinel-gateway"]
```

**Note:** With rustls (no OpenSSL linking), the runtime image only needs ca-certificates. Could use `FROM scratch` but lose shell access for debugging.

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| HTTP Framework | axum 0.8 | Actix Web 4 | Own runtime/actor model adds complexity; axum's Tower integration is cleaner for middleware composition |
| HTTP Framework | axum 0.8 | Warp | Filter-based API produces unreadable type errors; Warp is effectively unmaintained vs axum |
| Database | sqlx 0.8 | Diesel 2 | DSL learning curve + proc macro compile times; sqlx's raw SQL with compile-time checks is simpler for this use case |
| Database | sqlx 0.8 | SeaORM | ORM abstraction unnecessary for audit logs + config reads; adds dependency weight |
| MCP Protocol | rmcp 0.16 | Hand-rolled JSON-RPC | MCP is more than JSON-RPC (capabilities, schemas, session mgmt); rmcp provides spec-compliant types |
| MCP Protocol | rmcp 0.16 | jsonrpc-core 18 | jsonrpc-core is transport-agnostic JSON-RPC only, doesn't cover MCP-specific protocol layers |
| Rate Limiting | governor 0.10 | tower built-in RateLimit | Tower's RateLimit is per-service, not keyed. governor supports per-key (client+tool) limiting |
| Config | toml + serde | config crate | config crate adds format layering complexity; TOML + env vars is sufficient |
| TLS | rustls | OpenSSL | Pure Rust, no system dependency, simpler Docker builds, FIPS via aws_lc_rs |
| JWT | jsonwebtoken 10 | jwt-simple | jsonwebtoken is more mature, more downloads, better documented |

## What NOT to Use

| Technology | Why Not |
|------------|---------|
| OpenSSL | System dependency complicates Docker builds. rustls is pure Rust and sufficient. |
| Diesel | Heavy ORM for simple schemas. Compile-time cost of proc macros not justified. |
| Actix Web | Different async runtime model. Mixing tokio ecosystems adds friction. |
| tonic (gRPC) | MCP uses JSON-RPC over HTTP/stdio, not gRPC. Wrong protocol. |
| Redis | Rate limit state can live in-memory (governor) for v1 single-instance. Postgres for persistence. Adding Redis is premature for single-instance deployment. |
| OpenTelemetry | Explicitly deferred to v2 per project scope. tracing + Postgres audit logs sufficient for v1. |
| OPA | Policy-as-code engine deferred to v2. Simple TOML-based RBAC config for v1. |

## Full Dependency List

### Cargo.toml (Core)

```toml
[dependencies]
# Async runtime
tokio = { version = "1.47", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }

# MCP protocol
rmcp = { version = "0.16", features = ["server", "client", "transport-streamable-http-server", "transport-streamable-http-client"] }

# HTTP server
axum = { version = "0.8", features = ["json", "macros"] }
tower = { version = "0.5", features = ["full"] }
tower-http = { version = "0.6", features = ["cors", "trace", "timeout", "compression-gzip"] }

# HTTP client
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json", "stream", "http2"] }

# Auth
jsonwebtoken = { version = "10", features = ["aws_lc_rs"] }

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "uuid", "chrono", "json"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Configuration
toml = "0.8"
dotenvy = "0.15"
clap = { version = "4.5", features = ["derive", "env"] }

# Rate limiting
governor = "0.10"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"

# Channels & concurrency
flume = "0.12"
arc-swap = "1"

# Error handling
thiserror = "2"
anyhow = "1"

# Performance
mimalloc = { version = "0.1", default-features = false }
bytes = "1"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
futures = "0.3"

[dev-dependencies]
wiremock = "0.6"
testcontainers = "0.23"
tokio-test = "0.4"
```

### Build Profile

```toml
[profile.release]
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```

**Rationale:** `lto = "fat"` + `codegen-units = 1` maximizes runtime performance at the cost of longer compile times (acceptable for release builds). `strip = true` reduces binary size. `panic = "abort"` eliminates unwind tables (~10% smaller binary). This matches the existing wrapper's build profile.

## Version Pinning Strategy

- **Pin major.minor** in Cargo.toml (e.g., `"0.8"` not `"0.8.6"`). Cargo.lock pins exact versions.
- **Exception:** rmcp -- pin exact version (`"=0.16.0"`) because it's pre-1.0 with frequent breaking changes.
- **LTS preference:** Use tokio 1.47.x LTS (supported until Sept 2026) rather than bleeding-edge 1.49.

## Sources

- [Axum 0.8.0 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) -- axum v0.8 release notes
- [rmcp on crates.io](https://crates.io/crates/rmcp) -- official MCP Rust SDK, v0.16.0
- [rmcp GitHub](https://github.com/modelcontextprotocol/rust-sdk) -- MCP spec 2025-11-25 implementation
- [sqlx on GitHub](https://github.com/launchbadge/sqlx) -- v0.8.6 stable
- [governor on GitHub](https://github.com/boinkor-net/governor) -- v0.10.2, GCRA rate limiting
- [jsonwebtoken on crates.io](https://crates.io/crates/jsonwebtoken) -- v10.3.0
- [tokio versions](https://crates.io/crates/tokio/versions) -- LTS 1.47.x until Sept 2026
- [Rust wrapper analysis](/home/lwb3/sentinel-gateway/docs/RUST-WRAPPER-ANALYSIS.md) -- existing crate choices validated
