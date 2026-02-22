# Phase 5: Audit Logging - Research

**Researched:** 2026-02-22
**Domain:** Async Postgres audit logging with embedded migrations in Rust
**Confidence:** HIGH

## Summary

Phase 5 adds structured audit logging to Postgres for every `tools/call` request. The core challenge is inserting audit records asynchronously (fire-and-forget via `tokio::spawn`) so that slow or unavailable Postgres never blocks tool call responses. The secondary challenge is running embedded SQL migrations at startup so there is never a manual schema step.

The Rust ecosystem has a clear standard for this: **sqlx** for async Postgres with embedded migrations, **uuid** for request IDs. Both are mature, well-documented, and already aligned with the project's tokio runtime. The config already has `[postgres]` and `gateway.audit_enabled` fields -- no config changes needed.

**Primary recommendation:** Use sqlx 0.8 with PgPool, embedded `migrate!()` macro at startup, and a bounded `mpsc` channel feeding a `tokio::spawn`ed writer task for non-blocking audit inserts.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AUDIT-01 | Log every tool call to Postgres with: timestamp, client identity, tool name, backend, request args (redactable), response status, latency | AuditEntry struct + INSERT query with all fields; redaction via serde skip or truncation |
| AUDIT-02 | Assign unique request ID to each tool call, included in all log entries | uuid::Uuid::new_v4() generated at start of each tools/call handler |
| AUDIT-03 | Audit logging is async and does not block request processing | Bounded mpsc channel + background writer task pattern |
| DEPLOY-04 | Database schema migrations run automatically on gateway startup | sqlx::migrate!() macro with embedded SQL files |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| sqlx | 0.8.6 | Async Postgres driver + migrations | Only pure-Rust async SQL toolkit; compile-time query checking; built-in migration system |
| uuid | 1.x | Request UUID generation | De-facto standard; v4 random UUIDs for unique request IDs |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4 | Timestamp types for audit entries | Maps to Postgres TIMESTAMPTZ via sqlx chrono feature |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| sqlx | tokio-postgres | Lower-level, no migration system, more boilerplate |
| sqlx | diesel | Heavier ORM, doesn't align with project's "thin wrapper" approach |
| chrono | time | sqlx defaults to `time` crate; chrono is more ergonomic and widely used |
| uuid v4 | ULID | ULIDs are sortable but add a dependency; UUIDs are sufficient for audit logs |

**Installation (Cargo.toml additions):**
```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "migrate", "chrono", "uuid"] }
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```

## Architecture Patterns

### Recommended Module Structure
```
src/
├── audit/
│   ├── mod.rs          # pub mod + re-exports
│   ├── db.rs           # PgPool creation, migration runner, insert query
│   └── writer.rs       # Background writer task (channel consumer)
migrations/
└── 001_create_audit_log.sql
```

### Pattern 1: Async Audit Writer (Channel + Background Task)
**What:** A bounded mpsc channel decouples the dispatch loop from Postgres writes. The dispatch loop sends `AuditEntry` structs into the channel; a background tokio task drains the channel and executes INSERTs.
**When to use:** Always -- this is the core pattern for AUDIT-03.
**Example:**
```rust
// Source: project architecture decision (bounded channels everywhere)
use tokio::sync::mpsc;

pub struct AuditEntry {
    pub request_id: uuid::Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub client_subject: String,
    pub client_role: String,
    pub tool_name: String,
    pub backend_name: String,
    pub request_args: Option<serde_json::Value>,
    pub response_status: String, // "success" | "error"
    pub error_message: Option<String>,
    pub latency_ms: i64,
}

pub async fn audit_writer(
    pool: sqlx::PgPool,
    mut rx: mpsc::Receiver<AuditEntry>,
) {
    while let Some(entry) = rx.recv().await {
        if let Err(e) = insert_audit_entry(&pool, &entry).await {
            tracing::error!(error = %e, "Failed to write audit log");
            // Never panic -- audit failure must not crash the gateway
        }
    }
    tracing::info!("Audit writer shutting down");
}
```

### Pattern 2: Embedded Migrations at Startup
**What:** SQL migration files in `migrations/` directory are embedded into the binary at compile time via `sqlx::migrate!()`. Run on startup before accepting requests.
**When to use:** DEPLOY-04 -- migrations run automatically.
**Example:**
```rust
// Source: https://docs.rs/sqlx/latest/sqlx/macro.migrate.html
pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    tracing::info!("Database migrations complete");
    Ok(())
}
```

### Pattern 3: Request UUID Propagation
**What:** Generate a UUID at the start of each `tools/call` handler. Pass it through the dispatch flow and include it in the audit entry.
**When to use:** AUDIT-02 -- every log entry has a unique request ID.
**Example:**
```rust
let request_id = uuid::Uuid::new_v4();
let start = std::time::Instant::now();
// ... handle tool call ...
let latency_ms = start.elapsed().as_millis() as i64;
// Send audit entry with request_id, latency, result status
```

### Pattern 4: PgPool Initialization with Config
**What:** Create a PgPool from the existing `[postgres]` config section. The `url_env` and `max_connections` fields are already defined.
**Example:**
```rust
use sqlx::postgres::PgPoolOptions;

pub async fn create_pool(url: &str, max_connections: u32) -> Result<sqlx::PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await
}
```

### Anti-Patterns to Avoid
- **Blocking inserts in dispatch loop:** Never `pool.execute().await` inline in `run_dispatch` -- always send to the channel
- **Unbounded channels:** Project decision says "bounded channels everywhere" -- use a reasonable buffer (e.g., 1024)
- **Panicking on audit failure:** Audit is best-effort; a Postgres outage must not crash the gateway
- **Storing full response bodies:** Only store status and error message, not full response payloads (storage bloat)

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Database migrations | Custom SQL runner | `sqlx::migrate!()` | Tracks applied migrations in `_sqlx_migrations` table; handles idempotency |
| Connection pooling | Manual connection management | `sqlx::PgPool` | Handles reconnect, idle timeout, max connections |
| UUID generation | Random string IDs | `uuid::Uuid::new_v4()` | RFC 4122 compliant, proper entropy |
| Timestamp handling | Manual epoch math | `chrono::Utc::now()` | Timezone-safe, maps directly to Postgres TIMESTAMPTZ |

**Key insight:** sqlx handles the entire lifecycle (pool creation, migrations, query execution, type mapping) in one crate. Adding separate migration tools or connection managers would introduce unnecessary seams.

## Common Pitfalls

### Pitfall 1: sqlx compile-time query checking requires DATABASE_URL
**What goes wrong:** `sqlx::query!()` macros require a live database at compile time to verify SQL.
**Why it happens:** The macro connects to Postgres during `cargo build` to validate queries.
**How to avoid:** Use `sqlx::query()` (runtime, not macro) OR set up `sqlx-data.json` offline mode OR use `query_as` with explicit types. For this project, **use runtime `sqlx::query()` with `.bind()` -- simpler, no compile-time DB dependency.**
**Warning signs:** Build fails with "DATABASE_URL must be set" errors.

### Pitfall 2: Channel backpressure when Postgres is slow
**What goes wrong:** If Postgres is slow/down, the bounded channel fills up and `tx.send()` blocks the dispatch loop.
**Why it happens:** Bounded channel is correct (prevents unbounded memory growth) but blocking is not acceptable.
**How to avoid:** Use `tx.try_send()` instead of `tx.send()`. If the channel is full, log a warning and drop the audit entry. Audit is best-effort.
**Warning signs:** Dispatch loop latency spikes correlated with Postgres latency.

### Pitfall 3: Migration file ordering
**What goes wrong:** Migrations run out of order if filenames don't sort correctly.
**Why it happens:** sqlx sorts migration files lexicographically.
**How to avoid:** Prefix with zero-padded numbers: `001_`, `002_`, etc.

### Pitfall 4: PgPool creation blocks startup indefinitely
**What goes wrong:** `PgPool::connect()` waits forever if Postgres is unreachable.
**Why it happens:** Default connection timeout is generous.
**How to avoid:** Use `PgPoolOptions::new().acquire_timeout(Duration::from_secs(5))` and handle the error gracefully. Consider: should the gateway start without audit (degraded mode) or fail fast? Recommendation: **fail fast** -- if audit is enabled in config but Postgres is unreachable, refuse to start.

### Pitfall 5: Redactable request args
**What goes wrong:** Storing raw tool arguments may log sensitive data (SQL queries, API keys in args).
**Why it happens:** AUDIT-01 says "request args (redactable)" -- the schema needs to support this.
**How to avoid:** Store args as `JSONB` but add a `redacted` boolean column. In future, specific tools can be configured to redact args. For v1, store everything (the user controls what tools are called).

## Code Examples

### Migration SQL (001_create_audit_log.sql)
```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    request_id UUID NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    client_subject TEXT NOT NULL,
    client_role TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    backend_name TEXT NOT NULL,
    request_args JSONB,
    response_status TEXT NOT NULL,
    error_message TEXT,
    latency_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_log_timestamp ON audit_log (timestamp);
CREATE INDEX idx_audit_log_request_id ON audit_log (request_id);
CREATE INDEX idx_audit_log_client_subject ON audit_log (client_subject);
CREATE INDEX idx_audit_log_tool_name ON audit_log (tool_name);
```

### Insert Query
```rust
// Source: sqlx runtime query pattern (no compile-time DB dependency)
async fn insert_audit_entry(
    pool: &sqlx::PgPool,
    entry: &AuditEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO audit_log
           (request_id, timestamp, client_subject, client_role, tool_name,
            backend_name, request_args, response_status, error_message, latency_ms)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#
    )
    .bind(entry.request_id)
    .bind(entry.timestamp)
    .bind(&entry.client_subject)
    .bind(&entry.client_role)
    .bind(&entry.tool_name)
    .bind(&entry.backend_name)
    .bind(&entry.request_args)
    .bind(&entry.response_status)
    .bind(&entry.error_message)
    .bind(entry.latency_ms)
    .execute(pool)
    .await?;
    Ok(())
}
```

### Integration into run_dispatch (sketch)
```rust
// In gateway.rs tools/call handler:
let request_id = uuid::Uuid::new_v4();
let start = std::time::Instant::now();

// ... existing tool call logic ...

let latency_ms = start.elapsed().as_millis() as i64;
let entry = AuditEntry {
    request_id,
    timestamp: chrono::Utc::now(),
    client_subject: caller.subject.clone(),
    client_role: caller.role.clone(),
    tool_name: name.to_string(),
    backend_name: backend_name.clone(),
    request_args: params.clone(),
    response_status: if resp.error.is_some() { "error" } else { "success" }.to_string(),
    error_message: resp.error.as_ref().map(|e| e.message.clone()),
    latency_ms,
};
if let Err(_) = audit_tx.try_send(entry) {
    tracing::warn!("Audit channel full, dropping entry");
}
```

### Startup Sequence in main.rs
```rust
// After config load, before dispatch:
let pool = audit::db::create_pool(&pg_url, config.postgres.max_connections).await?;
audit::db::run_migrations(&pool).await?;
let (audit_tx, audit_rx) = mpsc::channel::<AuditEntry>(1024);
tokio::spawn(audit::writer::audit_writer(pool, audit_rx));
// Pass audit_tx into run_dispatch
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| sqlx compile-time macros only | Runtime queries fully supported | sqlx 0.7+ | No DATABASE_URL needed at build time |
| Separate migration CLI required | `migrate!()` macro embeds in binary | sqlx 0.6+ | Zero external tooling for migrations |
| tokio-postgres + deadpool | sqlx PgPool built-in | sqlx 0.3+ | One crate handles pool + queries |

## Open Questions

1. **Audit channel buffer size**
   - What we know: Must be bounded (project decision). 1024 is reasonable default.
   - What's unclear: Whether real workload justifies more or less.
   - Recommendation: Start with 1024, make configurable later if needed.

2. **Graceful shutdown flush**
   - What we know: Phase 7 (HEALTH-05) requires "flush audit logs" on SIGTERM.
   - What's unclear: Phase 5 doesn't own shutdown logic yet.
   - Recommendation: Design the writer to drain the channel on shutdown (when rx closes, drain remaining entries before exiting). Phase 7 will hook into this.

3. **run_dispatch signature change**
   - What we know: `run_dispatch` currently takes 7 parameters. Adding `audit_tx` makes 8.
   - What's unclear: Whether to refactor into a context struct.
   - Recommendation: Add `audit_tx: Option<mpsc::Sender<AuditEntry>>` parameter. Optional so existing tests work without Postgres. Refactor to context struct is a future cleanup.

## Sources

### Primary (HIGH confidence)
- [sqlx docs](https://docs.rs/sqlx/latest/sqlx/) - Version 0.8.6 confirmed, features verified
- [sqlx postgres types](https://docs.rs/sqlx/latest/sqlx/postgres/types/index.html) - chrono/uuid feature flags verified
- [sqlx migrate macro](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html) - Embedded migration pattern confirmed
- [uuid crate](https://crates.io/crates/uuid) - Version 1.21.0, v4 feature for random UUIDs

### Secondary (MEDIUM confidence)
- [sqlx GitHub](https://github.com/launchbadge/sqlx) - Runtime query support, PgPool patterns
- Existing codebase analysis - Config types, gateway dispatch, bounded channel convention

### Tertiary (LOW confidence)
- None -- all findings verified with primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - sqlx is the de-facto async Postgres driver for Rust/tokio; uuid is universal
- Architecture: HIGH - Channel + background writer is the standard non-blocking write pattern in tokio; matches project's existing bounded-channel convention
- Pitfalls: HIGH - compile-time query checking gotcha is well-documented; channel backpressure is standard concurrency concern

**Research date:** 2026-02-22
**Valid until:** 2026-04-22 (stable ecosystem, 60-day validity)
