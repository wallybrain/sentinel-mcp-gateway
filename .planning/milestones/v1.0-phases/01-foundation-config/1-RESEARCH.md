# Phase 1: Foundation & Config - Research

**Researched:** 2026-02-22
**Domain:** Rust project scaffold, TOML configuration, JSON-RPC 2.0 types, request ID remapping
**Confidence:** HIGH

## Summary

Phase 1 establishes the compiling Rust binary, configuration system, and foundational JSON-RPC types that every subsequent phase builds on. The scope is deliberately narrow: no networking, no transport, no MCP protocol -- just a binary that loads config, defines types, and proves ID remapping correctness via tests.

The existing Rust wrapper at `/home/lwb3/mcp-context-forge/tools_rust/wrapper/` provides proven patterns for clap CLI, tracing logging, mimalloc allocation, and JSON-RPC ID parsing. These patterns should be adapted (not copied verbatim) since the gateway has different concerns (TOML config vs CLI-only, multi-backend routing vs single-endpoint forwarding).

**Primary recommendation:** Define JSON-RPC 2.0 types from scratch using serde (not rmcp, not jsonrpc-core) for maximum control. Use `toml` + `serde` for config deserialization into typed structs. Use `AtomicU64` for monotonic ID generation in the remapping layer.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PROTO-01 | Gateway implements JSON-RPC 2.0 (request/response correlation, error objects, notifications) | Custom JSON-RPC types with serde, covering Request/Response/Notification/Error per spec. Code examples below. |
| PROTO-04 | Gateway remaps JSON-RPC request IDs to prevent collisions between backends | AtomicU64 counter + HashMap remapping table. Unit tests for concurrent ID generation and round-trip remapping. |
| CONFIG-01 | All gateway behavior is configured via a single `sentinel.toml` file | `toml` crate + `serde::Deserialize` structs. Fail-fast on parse error with actionable message. |
| CONFIG-02 | Config includes: auth settings, backend definitions, role-to-tool mappings, rate limits, kill switches | Typed config structs with nested tables. Full schema documented below. |
| CONFIG-04 | Secrets injected via environment variables, never in config file | Config references env var NAMES (e.g., `jwt_secret_env = "JWT_SECRET_KEY"`), resolved at startup via `std::env::var`. |
| DEPLOY-01 | Gateway builds as a single Rust binary via `cargo build --release` | Standard Cargo project with release profile (LTO, single codegen unit, strip, panic=abort). |
</phase_requirements>

## Standard Stack

### Core (Phase 1 only)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.47 | Async runtime | LTS until Sept 2026. Required by all downstream phases. Use `features = ["full"]` for Phase 1 simplicity. |
| serde | 1 | Serialization framework | Universal. Every config struct and JSON-RPC type derives `Serialize`/`Deserialize`. |
| serde_json | 1 | JSON parsing | JSON-RPC messages are JSON. Used for parsing, serializing, and raw `Value` manipulation. |
| toml | 0.9 | TOML config parsing | Standard Rust TOML crate. Wrapper already uses 0.9.11. Newer than stack research noted (was 0.8.x). |
| clap | 4.5 | CLI argument parsing | Derive macros + env var bindings. Proven in wrapper. Minimal CLI for Phase 1: `--config` path and `--log-level`. |
| tracing | 0.1 | Structured logging | Industry standard. Phase 1 uses it for startup/config errors only. |
| tracing-subscriber | 0.3 | Log output | EnvFilter for log level control. stderr output for Phase 1 (no file appender needed yet). |
| thiserror | 2 | Error types | Derive `Error` impls for gateway-specific error enums. |
| anyhow | 1 | Application errors | Top-level error handling in main. Provides context chains for startup failures. |
| mimalloc | 0.1 | Global allocator | Drop-in performance. Proven in wrapper. |
| dotenvy | 0.15 | .env loading | Load secrets from `.env` file during development. Proven in wrapper. |

### Not Needed in Phase 1

| Library | When Needed | Why Not Now |
|---------|-------------|-------------|
| rmcp | Phase 2 | MCP protocol types needed for initialize/tools/list, not for Phase 1 JSON-RPC foundation |
| axum | Phase 7+ | HTTP server for health endpoints, not needed in foundation |
| reqwest | Phase 3 | HTTP client for backends, not needed in foundation |
| sqlx | Phase 5 | Database, not needed in foundation |
| jsonwebtoken | Phase 4 | JWT validation, not needed in foundation |
| governor | Phase 6 | Rate limiting, not needed in foundation |
| flume | Phase 2 | Channels for transport, not needed in foundation |
| arc-swap | Phase 2+ | Hot-swap config, not needed until config reload (Phase 9) |

**Installation (Phase 1 Cargo.toml):**
```toml
[package]
name = "sentinel-gateway"
version = "0.1.0"
edition = "2024"
license = "Proprietary"

[dependencies]
tokio = { version = "1.47", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
clap = { version = "4.5", features = ["derive", "env"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2"
anyhow = "1"
mimalloc = { version = "0.1", default-features = false }
dotenvy = "0.15"

[dev-dependencies]
tokio-test = "0.4"

[profile.release]
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```

## Architecture Patterns

### Recommended Project Structure

```
sentinel-gateway/
├── Cargo.toml
├── sentinel.toml              # Example config (committed, uses placeholders)
├── .env.example               # Example env vars (committed, placeholder values)
├── .env                       # Real secrets (never committed)
├── .gitignore
├── CLAUDE.md
├── src/
│   ├── main.rs                # Entry point: mimalloc, tokio::main, init
│   ├── lib.rs                 # Module re-exports
│   ├── cli.rs                 # clap CLI args (--config, --log-level)
│   ├── config/
│   │   ├── mod.rs             # Config struct, load_config(), validate()
│   │   ├── types.rs           # Nested config types (BackendConfig, RbacConfig, etc.)
│   │   └── secrets.rs         # Env var resolution for secrets
│   ├── protocol/
│   │   ├── mod.rs             # Re-exports
│   │   ├── jsonrpc.rs         # JSON-RPC 2.0 types (Request, Response, Error, Notification)
│   │   └── id_remapper.rs     # Request ID remapping (AtomicU64 + HashMap)
│   └── logging.rs             # tracing setup
├── tests/
│   ├── config_test.rs         # Config loading, validation, secret resolution
│   └── id_remap_test.rs       # ID remapping correctness, concurrency
└── docs/
    └── (existing docs)
```

### Pattern 1: Typed Config with Serde

**What:** Deserialize TOML directly into strongly-typed Rust structs. Compiler catches config shape errors.
**When:** Always. Never use raw `toml::Value` for config.

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SentinelConfig {
    pub gateway: GatewayConfig,
    pub auth: AuthConfig,
    pub postgres: PostgresConfig,
    #[serde(default)]
    pub backends: Vec<BackendConfig>,
    #[serde(default)]
    pub rbac: RbacConfig,
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
    #[serde(default)]
    pub kill_switch: KillSwitchConfig,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub audit_enabled: bool,
}

fn default_listen() -> String { "127.0.0.1:9200".to_string() }
fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }
```

### Pattern 2: Env Var Secret Resolution

**What:** Config references env var NAMES, not values. Secrets resolved at startup.
**When:** JWT key, Postgres URL, API keys for stdio backend env vars.
**Why:** Secrets never touch the config file. `.env` is gitignored.

```rust
#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    /// Name of the env var containing the JWT secret (NOT the secret itself)
    pub jwt_secret_env: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
}

impl AuthConfig {
    pub fn resolve_jwt_secret(&self) -> Result<String, ConfigError> {
        std::env::var(&self.jwt_secret_env).map_err(|_| {
            ConfigError::MissingSecret {
                env_var: self.jwt_secret_env.clone(),
                context: "JWT secret key".to_string(),
            }
        })
    }
}
```

### Pattern 3: JSON-RPC 2.0 Types (Hand-Rolled, Not Library)

**What:** Own the JSON-RPC types. No dependency on `jsonrpc-core` or `rmcp` for foundational types.
**When:** Phase 1 and all subsequent phases.
**Why:** `jsonrpc-core` (18.0) is a full server framework, not just types. rmcp couples MCP semantics into JSON-RPC. The gateway needs precise control over serialization (e.g., omitting `id` for notifications, preserving raw `params` as `serde_json::Value` for pass-through routing).

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request ID. Can be string, number, or null.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(u64),
    String(String),
    Null,
}

/// Incoming JSON-RPC 2.0 message (request or notification).
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<JsonRpcId>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// True if this is a notification (no id field).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// Outgoing JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// Standard error codes
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;
```

### Pattern 4: Request ID Remapping with AtomicU64

**What:** Gateway assigns monotonic IDs to outbound requests, maps them back to original client IDs on response.
**When:** Every request forwarded to a backend.
**Why:** Prevents ID collision when multiple backends independently assign IDs. Critical for correctness.

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct IdRemapper {
    counter: AtomicU64,
    /// Maps gateway-assigned ID -> (original client ID, backend name)
    mappings: Mutex<HashMap<u64, (JsonRpcId, String)>>,
}

impl IdRemapper {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            mappings: Mutex::new(HashMap::new()),
        }
    }

    /// Remap an incoming client request ID to a unique gateway ID.
    /// Returns the gateway ID to use when forwarding to the backend.
    pub fn remap(&self, original_id: JsonRpcId, backend: &str) -> u64 {
        let gateway_id = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut map = self.mappings.lock().expect("lock poisoned");
        map.insert(gateway_id, (original_id, backend.to_string()));
        gateway_id
    }

    /// Restore the original client ID from a backend response.
    /// Returns None if the gateway ID is not found (stale or spurious response).
    pub fn restore(&self, gateway_id: u64) -> Option<(JsonRpcId, String)> {
        let mut map = self.mappings.lock().expect("lock poisoned");
        map.remove(&gateway_id)
    }

    /// Number of pending (unresolved) mappings.
    pub fn pending_count(&self) -> usize {
        let map = self.mappings.lock().expect("lock poisoned");
        map.len()
    }
}
```

### Pattern 5: Fail-Fast Config Loading

**What:** Binary exits immediately with a clear error if config is missing, malformed, or missing required secrets.
**When:** Startup, before any async runtime work.

```rust
pub fn load_config(path: &str) -> Result<SentinelConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;

    let config: SentinelConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {path}"))?;

    config.validate()?;
    Ok(config)
}

impl SentinelConfig {
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        // Check required env vars exist
        self.auth.resolve_jwt_secret()?;
        self.postgres.resolve_url()?;

        // Check backends have unique names
        let mut names = std::collections::HashSet::new();
        for backend in &self.backends {
            if !names.insert(&backend.name) {
                anyhow::bail!("Duplicate backend name: {}", backend.name);
            }
        }

        // Check RBAC references valid backends
        // (more validation as config schema grows)

        Ok(())
    }
}
```

### Anti-Patterns to Avoid

- **Using `jsonrpc-core` crate for types:** It's a full server framework with its own runtime model. We only need types. Own them.
- **Using `rmcp` in Phase 1:** rmcp couples MCP semantics (initialize, capabilities, tools) into JSON-RPC. Phase 1 is pure JSON-RPC -- MCP comes in Phase 2.
- **Storing secrets in TOML:** Config file references env var names. The actual secret values come from `std::env::var()` at runtime.
- **Using `serde_json::Value` for config:** Always deserialize into typed structs. Compiler catches schema mistakes.
- **Unbounded anything:** No unbounded channels, no unbounded HashMaps without cleanup. The ID remapper's HashMap grows with pending requests and shrinks as responses arrive.
- **Running `cargo build` in sandbox:** Rust builds fail in bwrap sandbox due to loopback permission errors. Always use `dangerouslyDisableSandbox: true`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML parsing | Custom parser | `toml` 0.9 + serde `Deserialize` | 5 lines to parse an entire config file into typed structs |
| CLI argument parsing | Manual `std::env::args` | `clap` 4.5 derive | Env var bindings, help text, validation for free |
| Structured logging | `println!`/`eprintln!` | `tracing` + `tracing-subscriber` | Structured fields, log levels, env-filter, async-safe |
| Error chains | String errors | `thiserror` (types) + `anyhow` (main) | `.context()` chains, `?` operator, proper Display/Debug |
| JSON serialization | Manual string building | `serde_json` | Correct escaping, null handling, performance |

## Common Pitfalls

### Pitfall 1: JSON-RPC ID Type Polymorphism

**What goes wrong:** JSON-RPC 2.0 allows `id` to be a string, number, or null. Code that assumes `id` is always a number will panic or silently corrupt IDs when a client sends string IDs.
**Why it happens:** Most examples show numeric IDs. But Claude Code and other MCP clients may use string UUIDs as request IDs.
**How to avoid:** Use `#[serde(untagged)] enum JsonRpcId { Number(u64), String(String), Null }`. The `untagged` deserializer tries each variant in order.
**Warning signs:** Tests only use numeric IDs. The wrapper's fast parser already handles this (`to_id` function handles `ValueInt` and `ValueString`).

### Pitfall 2: Notification vs Request Confusion

**What goes wrong:** JSON-RPC notifications have no `id` field. If code assumes every message has an `id`, it will error on notifications like `initialized`, `notifications/cancelled`, and `notifications/progress`.
**Why it happens:** Requests are more common, so code is written for the request case first and notifications are forgotten.
**How to avoid:** Make `id` an `Option<JsonRpcId>` in the request struct. Detect notifications by `id.is_none()`. Never add notifications to the ID remapper's pending map.
**Warning signs:** Errors when receiving `initialized` notification from client.

### Pitfall 3: Config Validation Gap

**What goes wrong:** Config parses successfully but contains logically invalid data: a backend with no tools mapped, an RBAC role referencing a non-existent backend, rate limits of 0, duplicate backend names.
**Why it happens:** Serde validates shape (types, required fields) but not semantics.
**How to avoid:** Implement a `validate()` method that runs after deserialization. Check cross-references, ranges, uniqueness constraints.
**Warning signs:** Gateway starts but behaves incorrectly because config has semantic errors.

### Pitfall 4: AtomicU64 Overflow

**What goes wrong:** The ID counter wraps around after 2^64 operations (~18 quintillion). In practice this will never happen, but if the counter starts at 0 and a response arrives with ID 0, it could conflict with a "no ID" sentinel value.
**Why it happens:** Using 0 as both "counter start" and "null/missing" value.
**How to avoid:** Start the counter at 1 (as shown in the code example). Reserve 0 for internal "no ID" if needed.
**Warning signs:** None in practice. This is defensive design, not a real-world risk at any conceivable request rate.

### Pitfall 5: TOML Array-of-Tables Syntax

**What goes wrong:** TOML has two ways to define arrays of objects: `[[backends]]` (array of tables) and `backends = [...]` (inline). The `[[backends]]` syntax is idiomatic for config files but developers unfamiliar with TOML try `[backends]` (single table, not array) and get a confusing error.
**Why it happens:** TOML syntax for arrays of tables is unusual compared to JSON/YAML.
**How to avoid:** Use `[[backends]]` in the example config. Document clearly in comments. The serde `Vec<BackendConfig>` type enforces the array requirement.
**Warning signs:** Config parse error "expected array, found table" on the backends section.

## Code Examples

### Complete sentinel.toml Example

```toml
[gateway]
listen = "127.0.0.1:9200"
log_level = "info"
audit_enabled = true

[auth]
jwt_secret_env = "JWT_SECRET_KEY"
jwt_issuer = "sentinel-gateway"
jwt_audience = "sentinel-api"

[postgres]
url_env = "DATABASE_URL"
max_connections = 10

[[backends]]
name = "n8n"
type = "http"
url = "http://mcp-n8n:3000"
timeout_secs = 60
retries = 3

[[backends]]
name = "sqlite"
type = "http"
url = "http://mcp-sqlite:3000"
timeout_secs = 60
retries = 3

[[backends]]
name = "context7"
type = "stdio"
command = "node"
args = ["/usr/local/lib/node_modules/@upstash/context7-mcp/dist/index.js"]
restart_on_exit = true
max_restarts = 5

[[backends]]
name = "firecrawl"
type = "stdio"
command = "node"
args = ["/usr/local/lib/node_modules/firecrawl-mcp/dist/index.js"]

[[backends]]
name = "exa"
type = "stdio"
command = "node"
args = ["/usr/local/lib/node_modules/exa-mcp-server/build/index.js"]
env = { "EXA_API_KEY_ENV" = "EXA_API_KEY" }

[[backends]]
name = "sequential-thinking"
type = "stdio"
command = "node"
args = ["/usr/local/lib/node_modules/@modelcontextprotocol/server-sequential-thinking/dist/index.js"]

[[backends]]
name = "playwright"
type = "stdio"
command = "node"
args = ["/usr/local/lib/node_modules/@anthropic/mcp-playwright/dist/index.js"]
env = {}

[rbac.roles.admin]
permissions = ["*"]

[rbac.roles.developer]
permissions = ["tools.read", "tools.execute"]
denied_tools = []

[rbac.roles.viewer]
permissions = ["tools.read"]

[rate_limits]
default_rpm = 1000

[rate_limits.per_tool]
execute_workflow = 10

[kill_switch]
disabled_tools = []
disabled_backends = []
```

### Complete Config Type Hierarchy

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct SentinelConfig {
    pub gateway: GatewayConfig,
    pub auth: AuthConfig,
    pub postgres: PostgresConfig,
    #[serde(default)]
    pub backends: Vec<BackendConfig>,
    #[serde(default)]
    pub rbac: RbacConfig,
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
    #[serde(default)]
    pub kill_switch: KillSwitchConfig,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub audit_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret_env: String,
    #[serde(default = "default_issuer")]
    pub jwt_issuer: String,
    #[serde(default = "default_audience")]
    pub jwt_audience: String,
}

#[derive(Debug, Deserialize)]
pub struct PostgresConfig {
    pub url_env: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub backend_type: BackendType,
    /// HTTP backend URL (required for http type)
    pub url: Option<String>,
    /// stdio command (required for stdio type)
    pub command: Option<String>,
    /// stdio command args
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for stdio backends (env var name -> env var name to resolve)
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default)]
    pub restart_on_exit: bool,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    #[serde(default = "default_health_interval")]
    pub health_interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    Http,
    Stdio,
}

#[derive(Debug, Default, Deserialize)]
pub struct RbacConfig {
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RoleConfig {
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rpm")]
    pub default_rpm: u32,
    #[serde(default)]
    pub per_tool: HashMap<String, u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct KillSwitchConfig {
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_backends: Vec<String>,
}

// Default functions
fn default_listen() -> String { "127.0.0.1:9200".to_string() }
fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }
fn default_issuer() -> String { "sentinel-gateway".to_string() }
fn default_audience() -> String { "sentinel-api".to_string() }
fn default_max_connections() -> u32 { 10 }
fn default_timeout() -> u64 { 60 }
fn default_retries() -> u32 { 3 }
fn default_max_restarts() -> u32 { 5 }
fn default_health_interval() -> u64 { 300 }
fn default_rpm() -> u32 { 1000 }
```

### main.rs Skeleton

```rust
use anyhow::Context;
use clap::Parser;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "sentinel-gateway", about = "MCP Gateway")]
struct Cli {
    /// Path to sentinel.toml config file
    #[arg(long, default_value = "sentinel.toml", env = "SENTINEL_CONFIG")]
    config: String,

    /// Override log level
    #[arg(long, env = "LOG_LEVEL")]
    log_level: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok(); // Load .env if present, ignore if missing

    let cli = Cli::parse();

    let config = sentinel_gateway::config::load_config(&cli.config)
        .context("Failed to load configuration")?;

    let log_level = cli.log_level.as_deref()
        .unwrap_or(&config.gateway.log_level);
    sentinel_gateway::logging::init(log_level);

    tracing::info!(config_path = %cli.config, "Sentinel Gateway starting");
    tracing::info!(backends = config.backends.len(), "Configuration loaded");

    // Phase 1: just load config and exit successfully.
    // Phase 2+ will add transport, protocol, and routing.
    tracing::info!("Foundation phase complete. No transport configured yet.");

    Ok(())
}
```

### ID Remapper Unit Test Examples

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_produces_unique_ids() {
        let remapper = IdRemapper::new();
        let id1 = remapper.remap(JsonRpcId::Number(1), "backend-a");
        let id2 = remapper.remap(JsonRpcId::Number(1), "backend-b");
        assert_ne!(id1, id2, "Same client ID to different backends must get different gateway IDs");
    }

    #[test]
    fn restore_returns_original_id() {
        let remapper = IdRemapper::new();
        let original = JsonRpcId::String("abc-123".to_string());
        let gateway_id = remapper.remap(original.clone(), "n8n");
        let (restored, backend) = remapper.restore(gateway_id).expect("mapping should exist");
        assert_eq!(restored, original);
        assert_eq!(backend, "n8n");
    }

    #[test]
    fn restore_removes_mapping() {
        let remapper = IdRemapper::new();
        let gateway_id = remapper.remap(JsonRpcId::Number(42), "sqlite");
        assert_eq!(remapper.pending_count(), 1);
        remapper.restore(gateway_id);
        assert_eq!(remapper.pending_count(), 0);
        assert!(remapper.restore(gateway_id).is_none(), "second restore should return None");
    }

    #[test]
    fn concurrent_remapping_no_collision() {
        use std::sync::Arc;
        use std::thread;

        let remapper = Arc::new(IdRemapper::new());
        let mut handles = vec![];

        for backend_idx in 0..10 {
            let r = Arc::clone(&remapper);
            handles.push(thread::spawn(move || {
                let mut ids = vec![];
                for req_idx in 0..100 {
                    let gid = r.remap(
                        JsonRpcId::Number(req_idx),
                        &format!("backend-{backend_idx}"),
                    );
                    ids.push(gid);
                }
                ids
            }));
        }

        let mut all_ids = vec![];
        for h in handles {
            all_ids.extend(h.join().unwrap());
        }

        // All 1000 gateway IDs must be unique
        let unique: std::collections::HashSet<_> = all_ids.iter().collect();
        assert_eq!(unique.len(), 1000, "All gateway IDs must be unique across concurrent remapping");
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `toml` 0.8.x | `toml` 0.9.x | 2025 | Wrapper already on 0.9.11. Use 0.9, not 0.8 as stack research noted. |
| `jsonrpc-core` 18 for types | Hand-rolled serde types | N/A (design decision) | jsonrpc-core is a server framework, not just types. Own the types for gateway control. |
| Rust edition 2021 | Rust edition 2024 | Rust 1.85 (Feb 2025) | Wrapper uses edition 2024. Match it. |
| `Mutex<HashMap>` for ID map | Could use `DashMap` | N/A | For Phase 1, `Mutex<HashMap>` is sufficient and simpler. DashMap adds dependency without benefit at this scale. Revisit in Phase 2+ if contention is measured. |

## Open Questions

1. **rmcp integration point**
   - What we know: rmcp 0.16 provides MCP protocol types (ToolInfo, CallToolResult, ServerCapabilities). Decision from STATE.md says "use for protocol types only, not server runtime."
   - What's unclear: Exactly which types from rmcp will wrap our JSON-RPC types in Phase 2. Will rmcp's `JsonRpcMessage` conflict with our own types?
   - Recommendation: Phase 1 defines its own JSON-RPC types. Phase 2 research will determine the rmcp integration boundary. If rmcp's types are incompatible, we keep ours and convert at the boundary.

2. **Config schema evolution**
   - What we know: Phase 1 defines the full schema (backends, RBAC, rate limits, kill switches) even though most sections are not used until later phases.
   - What's unclear: Will later phases need config fields not anticipated now?
   - Recommendation: Define the full schema now with `#[serde(default)]` on optional sections. New fields can be added with defaults without breaking existing configs.

3. **Test infrastructure**
   - What we know: Phase 1 needs unit tests for config parsing and ID remapping.
   - What's unclear: Whether to use `cargo test` alone or set up a more elaborate test harness.
   - Recommendation: Plain `cargo test` with `#[cfg(test)]` modules. No test framework beyond tokio-test. Integration tests in `tests/` directory for config file loading.

## Sources

### Primary (HIGH confidence)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification) -- Request/Response/Notification/Error format, ID rules
- [Rust wrapper source code](/home/lwb3/mcp-context-forge/tools_rust/wrapper/) -- Proven patterns for clap, tracing, mimalloc, JSON-RPC ID parsing
- [Rust wrapper Cargo.toml](/home/lwb3/mcp-context-forge/tools_rust/wrapper/Cargo.toml) -- Current crate versions (toml 0.9.11, tokio 1.49, serde 1.0.228)
- Project research: [STACK.md](/home/lwb3/sentinel-gateway/.planning/research/STACK.md), [ARCHITECTURE.md](/home/lwb3/sentinel-gateway/.planning/research/ARCHITECTURE.md), [PITFALLS.md](/home/lwb3/sentinel-gateway/.planning/research/PITFALLS.md)

### Secondary (MEDIUM confidence)
- [toml crate docs](https://docs.rs/toml/latest/toml/) -- TOML parsing with serde
- [serde documentation](https://serde.rs/) -- Derive macros, attributes, custom deserialization
- [clap derive documentation](https://docs.rs/clap/latest/clap/_derive/index.html) -- CLI argument parsing patterns

### Tertiary (LOW confidence)
- None. All Phase 1 technologies are well-established Rust crates with extensive documentation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all crates are well-established, versions verified against existing wrapper
- Architecture: HIGH -- project structure follows standard Rust conventions, patterns proven in wrapper
- JSON-RPC types: HIGH -- spec is stable, serde approach is standard
- ID remapping: HIGH -- AtomicU64 + HashMap is a straightforward concurrent pattern
- Config schema: MEDIUM -- schema will likely evolve as later phases reveal needs, but `#[serde(default)]` handles this gracefully
- Pitfalls: HIGH -- well-understood Rust patterns, verified against spec and wrapper code

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable domain, 30 days)
