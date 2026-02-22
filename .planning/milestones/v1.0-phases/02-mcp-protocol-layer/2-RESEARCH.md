# Phase 2: MCP Protocol Layer - Research

**Researched:** 2026-02-22
**Domain:** MCP protocol (initialize handshake, tool catalog aggregation, stdio transport)
**Confidence:** HIGH

## Summary

Phase 2 transforms the skeleton binary from Phase 1 into a working MCP server that Claude Code can connect to via stdio. The three requirements are tightly coupled: the gateway must read/write newline-delimited JSON-RPC on stdin/stdout (PROTO-06), handle the MCP initialize handshake and respond with capabilities (PROTO-02), and aggregate tool catalogs from stub backends into a unified `tools/list` response (PROTO-03).

The key architectural decision is that rmcp 0.16 provides all needed protocol types (`InitializeRequestParams`, `InitializeResult`, `ServerCapabilities`, `Tool`, `ListToolsResult`, etc.) with `default-features = false` -- no server runtime pulled in, just types. The gateway owns its own transport layer (tokio BufReader on stdin, BufWriter on stdout) and its own lifecycle state machine. This keeps the gateway in control of message routing while getting spec-compliant serde types for free.

**Primary recommendation:** Build three layers bottom-up: (1) stdio transport (line reader/writer), (2) MCP lifecycle state machine (Created -> Initializing -> Operational -> Closed), (3) tool catalog with hardcoded stub tools for testing. Test end-to-end by piping JSON into the binary.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PROTO-02 | Gateway handles MCP initialize handshake and responds with merged capabilities | rmcp types (`InitializeRequestParams`, `InitializeResult`, `ServerCapabilities`, `ProtocolVersion::V_2025_03_26`), lifecycle state machine pattern, version negotiation rules from MCP spec |
| PROTO-03 | Gateway handles `tools/list` by aggregating schemas from all backends into one catalog | rmcp `Tool` and `ListToolsResult` types, `ToolCatalog` aggregation pattern, name collision strategy |
| PROTO-06 | Gateway accepts MCP requests via stdio transport (newline-delimited JSON-RPC on stdin/stdout) | Tokio BufReader/BufWriter pattern, newline-delimited framing rules from MCP spec, stderr for logging |
</phase_requirements>

## Standard Stack

### Core (New Dependencies for Phase 2)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | =0.16.0 | MCP protocol types (no runtime) | Official Rust SDK from modelcontextprotocol org. Provides `InitializeRequestParams`, `InitializeResult`, `ServerCapabilities`, `Tool`, `ListToolsResult`, `ProtocolVersion`, etc. Pin exact version (pre-1.0). |
| tokio (already added) | 1.47 | Async runtime, stdin/stdout, channels | Already in Cargo.toml from Phase 1. Use `tokio::io::stdin()`, `tokio::io::stdout()`, `BufReader`, `BufWriter`. |
| serde_json (already added) | 1 | JSON parsing/serialization | Already in Cargo.toml. Used for line-by-line JSON-RPC parsing and response serialization. |

### rmcp Feature Configuration

```toml
rmcp = { version = "=0.16.0", default-features = false }
```

No features needed. With `default-features = false`, rmcp provides all model types (`rmcp::model::*`) without pulling in server/client runtime, transport layers, or macros. Core dependencies (serde, serde_json, thiserror) are unconditional and already in our tree.

### No New Supporting Libraries Needed

Phase 2 uses only existing dependencies plus rmcp for types. No new crates for transport, channels, or framing -- tokio's built-in `BufReader`/`BufWriter` on stdin/stdout handle line-delimited framing natively.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rmcp types | Hand-rolled MCP types | rmcp gives spec-compliant serde for free; hand-rolling risks divergence from spec |
| rmcp default features | rmcp with `server` feature | Server feature pulls in runtime we don't want; gateway owns its own event loop |
| tokio BufReader | tokio-util LinesCodec | LinesCodec adds a dependency; BufReader.read_line() is simpler and sufficient for newline-delimited JSON |
| Hardcoded stub catalog | Mock backend connections | Stubs are simpler for Phase 2; real backend connections come in Phase 3 |

## Architecture Patterns

### Recommended Module Structure

```
src/
  protocol/
    mod.rs              # existing, add new module exports
    jsonrpc.rs          # existing Phase 1 types
    id_remapper.rs      # existing Phase 1 ID remapper
    mcp.rs              # NEW: MCP-specific types, state machine, capability building
  transport/
    mod.rs              # NEW: transport module
    stdio.rs            # NEW: stdin reader + stdout writer
  catalog/
    mod.rs              # NEW: tool catalog aggregation
  lib.rs                # add transport, catalog modules
  main.rs              # wire up transport -> protocol -> catalog
```

### Pattern 1: MCP Lifecycle State Machine

**What:** An enum-based state machine that tracks where the gateway is in the MCP lifecycle.
**When to use:** Every incoming message is checked against the current state before processing.

```rust
// Source: MCP spec 2025-03-26 lifecycle section
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpState {
    Created,        // awaiting initialize request
    Initializing,   // initialize received, response sent, awaiting initialized notification
    Operational,    // normal operation: tools/list, tools/call, ping
    Closed,         // transport closed or shutdown
}

impl McpState {
    pub fn can_accept_method(&self, method: &str) -> bool {
        match self {
            McpState::Created => method == "initialize" || method == "ping",
            McpState::Initializing => {
                method == "notifications/initialized" || method == "ping"
            }
            McpState::Operational => true,
            McpState::Closed => false,
        }
    }
}
```

**Key rules from spec:**
- No requests allowed before `initialize` completes (respond with error -32002 "Server not initialized")
- After sending `initialize` response, only `initialized` notification and `ping` accepted
- After `initialized` notification, enter Operational -- all methods accepted
- `ping` is always accepted (even pre-initialization)

### Pattern 2: Stdio Transport (Reader + Writer)

**What:** Two async tasks -- one reads lines from stdin, one writes lines to stdout. Connected via bounded channels.
**When to use:** This is THE upstream transport for Claude Code.

```rust
// Source: MCP spec 2025-03-26 transports section
// "Messages are delimited by newlines, and MUST NOT contain embedded newlines."

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

// Reader task: stdin -> channel
async fn stdio_reader(tx: tokio::sync::mpsc::Sender<String>) {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,  // EOF -- client closed stdin
            Ok(_) => {
                let trimmed = line.trim_end().to_string();
                if !trimmed.is_empty() {
                    if tx.send(trimmed).await.is_err() {
                        break;  // receiver dropped
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "stdin read error");
                break;
            }
        }
    }
}

// Writer task: channel -> stdout
async fn stdio_writer(mut rx: tokio::sync::mpsc::Receiver<String>) {
    let stdout = tokio::io::stdout();
    let mut writer = BufWriter::new(stdout);
    while let Some(msg) = rx.recv().await {
        // Each message is one line, terminated by newline
        if writer.write_all(msg.as_bytes()).await.is_err() { break; }
        if writer.write_all(b"\n").await.is_err() { break; }
        if writer.flush().await.is_err() { break; }
    }
}
```

**Bounded channels:** Use `tokio::sync::mpsc::channel(64)` -- not unbounded. 64 is plenty for the request pipeline; if the writer stalls, backpressure propagates to the reader, which is correct behavior.

**stderr for logging:** MCP spec says "server MAY write UTF-8 strings to stderr for logging." This is already correct -- tracing-subscriber writes to stderr by default.

### Pattern 3: Initialize Handshake Handler

**What:** Parse the `initialize` request, build capabilities from the tool catalog, respond.
**When to use:** The first meaningful message in every MCP session.

```rust
use rmcp::model::{
    InitializeRequestParams, InitializeResult, ServerCapabilities,
    ProtocolVersion, Implementation, Tool, ListToolsResult,
};

fn handle_initialize(params: InitializeRequestParams) -> Result<InitializeResult, String> {
    // Version negotiation: we support 2025-03-26
    let supported = ProtocolVersion::V_2025_03_26;
    // If client sends a version we don't support, respond with ours
    // Client decides whether to continue or disconnect

    let capabilities = ServerCapabilities::builder()
        .enable_tools()
        .build();

    Ok(InitializeResult {
        protocol_version: supported,
        capabilities,
        server_info: Implementation {
            name: "sentinel-gateway".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        instructions: Some("Sentinel Gateway - governed MCP tool access".into()),
    })
}
```

### Pattern 4: Tool Catalog Aggregation

**What:** A `ToolCatalog` struct that merges tools from multiple sources into a single list.
**When to use:** When responding to `tools/list` requests.

```rust
use rmcp::model::Tool;
use std::collections::HashMap;

pub struct ToolCatalog {
    /// tool_name -> (tool_definition, backend_name)
    tools: HashMap<String, (Tool, String)>,
}

impl ToolCatalog {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    /// Register tools from a backend. Prefixes on collision.
    pub fn register_backend(&mut self, backend_name: &str, tools: Vec<Tool>) {
        for tool in tools {
            let name = tool.name.to_string();
            if self.tools.contains_key(&name) {
                // Name collision -- prefix with backend name
                let prefixed = format!("{}__{}", backend_name, name);
                tracing::warn!(
                    tool = %name,
                    backend = backend_name,
                    renamed = %prefixed,
                    "Tool name collision, prefixing"
                );
                self.tools.insert(prefixed, (tool, backend_name.to_string()));
            } else {
                self.tools.insert(name, (tool, backend_name.to_string()));
            }
        }
    }

    /// Get all tools for tools/list response
    pub fn all_tools(&self) -> Vec<Tool> {
        self.tools.values().map(|(t, _)| t.clone()).collect()
    }

    /// Lookup which backend owns a tool
    pub fn route(&self, tool_name: &str) -> Option<&str> {
        self.tools.get(tool_name).map(|(_, b)| b.as_str())
    }
}
```

**For Phase 2:** Populate with hardcoded stub tools (no real backend connections yet). This proves the aggregation logic. Phase 3 will replace stubs with real HTTP backend discovery.

### Pattern 5: Message Dispatch Loop

**What:** Central async loop that reads parsed messages and dispatches based on method + state.
**When to use:** The core event loop of the gateway.

```rust
// Simplified dispatch -- actual implementation will be more structured
async fn dispatch_loop(
    mut rx: tokio::sync::mpsc::Receiver<String>,
    tx: tokio::sync::mpsc::Sender<String>,
    catalog: &ToolCatalog,
) {
    let mut state = McpState::Created;

    while let Some(line) = rx.recv().await {
        // Parse as JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // Send parse error response
                let err = JsonRpcResponse::error(
                    JsonRpcId::Null,
                    PARSE_ERROR,
                    format!("Parse error: {}", e),
                );
                let _ = tx.send(serde_json::to_string(&err).unwrap()).await;
                continue;
            }
        };

        // State gate
        if !state.can_accept_method(&request.method) {
            if let Some(id) = &request.id {
                let err = JsonRpcResponse::error(
                    id.clone(),
                    -32002,
                    "Server not initialized".to_string(),
                );
                let _ = tx.send(serde_json::to_string(&err).unwrap()).await;
            }
            continue;
        }

        match request.method.as_str() {
            "initialize" => { /* handle init, transition state */ }
            "notifications/initialized" => { state = McpState::Operational; }
            "tools/list" => { /* return catalog.all_tools() */ }
            "tools/call" => { /* Phase 3+ */ }
            "ping" => { /* respond with empty result */ }
            _ => { /* method not found error */ }
        }
    }
    // stdin closed -- shutdown
    state = McpState::Closed;
}
```

### Anti-Patterns to Avoid

- **Deserializing into rmcp's `JsonRpcRequest` type:** rmcp has its own JSON-RPC types, but we already have ours from Phase 1. Use OUR `JsonRpcRequest` for transport parsing, then selectively deserialize `params` into rmcp model types only when needed. Don't mix two JSON-RPC type systems.
- **Using rmcp server runtime:** The decision is to use rmcp for types only. Don't pull in `ServerHandler` trait or `Service` -- we own the event loop.
- **Buffering entire stdin before processing:** Read line-by-line. stdin is a stream, not a batch.
- **Writing to stdout from multiple tasks without coordination:** Only the writer task writes to stdout. All response paths go through the writer channel.
- **Forgetting to handle notifications (no id field):** `initialized` is a notification, not a request. Don't try to send a response to it. The existing `JsonRpcRequest.is_notification()` method handles this.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP protocol types | Custom InitializeParams/Result/ServerCapabilities structs | `rmcp::model::*` | 20+ types with precise serde attributes matching spec; hand-rolling risks field name mismatches (`protocolVersion` vs `protocol_version`), missing optional fields |
| Protocol version constants | String literals like "2025-03-26" | `rmcp::model::ProtocolVersion::V_2025_03_26` | rmcp already has the constants and serde impl |
| Tool schema types | Custom Tool struct with inputSchema | `rmcp::model::Tool` | Tool has 9 fields including `annotations`, `execution`, `output_schema` -- easy to miss fields when hand-rolling |
| Newline-delimited framing | Custom codec or tokio-util LinesCodec | `BufReader::read_line()` | Built into tokio, zero additional dependencies, handles the exact framing MCP requires |

## Common Pitfalls

### Pitfall 1: Serde Field Name Mismatch Between Our Types and rmcp

**What goes wrong:** Our `JsonRpcRequest` uses `method: String` and `params: Option<Value>`, but rmcp types expect specific serde rename attributes (e.g., `protocolVersion` not `protocol_version` in JSON). If we deserialize `params` from our raw `Value` into rmcp types, field names must match the JSON wire format.
**Why it happens:** rmcp uses `#[serde(rename_all = "camelCase")]` on some types but not all. Our transport layer sees raw JSON; rmcp types expect their own serde conventions.
**How to avoid:** Deserialize `params` directly from the `serde_json::Value` using `serde_json::from_value::<InitializeRequestParams>(params)`. rmcp's serde attributes handle the field name mapping. Don't manually extract fields from the Value.

### Pitfall 2: Responding to Notifications

**What goes wrong:** The `initialized` notification has no `id` field. Sending a response to a notification violates JSON-RPC 2.0 spec and will confuse clients.
**Why it happens:** It's natural to respond to every incoming message. But notifications are fire-and-forget.
**How to avoid:** Check `request.is_notification()` (already implemented in Phase 1). For notifications, perform the side effect (state transition) but never send a response.

### Pitfall 3: State Machine Bypass

**What goes wrong:** A `tools/list` or `tools/call` arrives before `initialize` completes. Without the state gate, the gateway processes it and returns invalid results (empty catalog, no negotiated capabilities).
**Why it happens:** Claude Code or other clients might race requests.
**How to avoid:** The state machine MUST be the first check on every incoming message. Reject with error code -32002 "Server not initialized" for any non-initialize request in `Created` state. This is per MCP spec.

### Pitfall 4: Stdout Pollution

**What goes wrong:** Tracing logs or debug prints go to stdout, mixing with JSON-RPC responses. The client sees non-JSON lines and crashes.
**Why it happens:** Default tracing-subscriber can write to stdout. `println!` macros go to stdout.
**How to avoid:** Ensure tracing-subscriber writes to stderr (already configured in Phase 1). Never use `println!` -- only the writer task writes to stdout. Add a test that verifies no non-JSON content appears on stdout.

### Pitfall 5: Forgetting `\n` Flush After Each Response

**What goes wrong:** BufWriter buffers output. If you don't flush after each JSON-RPC response, the client hangs waiting for data that's sitting in the buffer.
**Why it happens:** BufWriter is designed to batch writes for performance. But MCP stdio requires each message to be delivered immediately.
**How to avoid:** Call `writer.flush().await` after every response write. Or use `writer.write_all(msg + "\n")` followed by explicit flush.

## Code Examples

### Complete Initialize Handshake (Verified Against MCP Spec 2025-03-26)

```rust
// Source: https://modelcontextprotocol.io/specification/2025-03-26/basic/lifecycle

// Client sends:
// {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{"roots":{"listChanged":true},"sampling":{}},"clientInfo":{"name":"claude-code","version":"1.0.0"}}}

// Gateway receives, parses params into rmcp type:
let params: InitializeRequestParams = serde_json::from_value(request.params.unwrap())?;

// Gateway builds response:
let result = InitializeResult {
    protocol_version: ProtocolVersion::V_2025_03_26,
    capabilities: ServerCapabilities::builder()
        .enable_tools()
        .build(),
    server_info: Implementation {
        name: "sentinel-gateway".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    },
    instructions: Some("Sentinel Gateway - governed MCP tool access".into()),
};

// Serialize as JSON-RPC response:
let response = JsonRpcResponse::success(
    request.id.unwrap(),
    serde_json::to_value(&result)?,
);
// -> {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"sentinel-gateway","version":"0.1.0"},"instructions":"..."}}

// Client then sends: {"jsonrpc":"2.0","method":"notifications/initialized"}
// Gateway transitions to McpState::Operational (no response sent)
```

### tools/list Response with Stub Catalog

```rust
// Source: https://modelcontextprotocol.io/specification/2025-03-26/server/tools

// Client sends: {"jsonrpc":"2.0","id":2,"method":"tools/list"}

let tools = catalog.all_tools();
let list_result = ListToolsResult::with_all_items(tools);
let response = JsonRpcResponse::success(
    request.id.unwrap(),
    serde_json::to_value(&list_result)?,
);
// -> {"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"stub_tool","description":"...","inputSchema":{...}}]}}
```

### Ping Response

```rust
// Source: MCP spec - ping is a simple request/response with empty result

// Client sends: {"jsonrpc":"2.0","id":3,"method":"ping"}

let response = JsonRpcResponse::success(
    request.id.unwrap(),
    serde_json::to_value(&serde_json::json!({}))?
);
// -> {"jsonrpc":"2.0","id":3,"result":{}}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| HTTP+SSE transport (2024-11-05) | Streamable HTTP (2025-03-26) | March 2025 | Not relevant to Phase 2 (stdio only), but matters for Phase 3+ |
| No JSON-RPC batching | Batching supported (2025-03-26) | March 2025 | Phase 2 does NOT need batch support (stdio is sequential); defer to later phase |
| No tasks/async ops | Tasks (2025-11-25 spec) | November 2025 | Out of scope for v1; `ProtocolVersion::V_2025_03_26` does not include tasks |

**Protocol version we target:** 2025-03-26 (matches rmcp `LATEST` constant). The 2025-11-25 spec adds Tasks, mandatory PKCE OAuth, and extensions -- all deferred to v2.

## Open Questions

1. **rmcp `Tool.name` is `Cow<'static, str>`**
   - What we know: rmcp uses `Cow<'static, str>` for tool names, which means either `&'static str` or owned `String`.
   - What's unclear: When deserializing from backend responses, the names will be `String` (owned). Need to verify `Cow::from(String)` works seamlessly with rmcp's serde.
   - Recommendation: Test deserialization in Phase 2 unit tests. If `Cow` causes issues, wrap `Tool` in a gateway-specific struct.

2. **Should `tools/list` support pagination?**
   - What we know: MCP spec supports optional `cursor` pagination. rmcp `ListToolsResult` has `next_cursor: Option<Cursor>`.
   - What's unclear: With ~40 tools total (19 from HTTP backends + ~20 from stdio), pagination is unnecessary.
   - Recommendation: Return all tools in one response (`ListToolsResult::with_all_items()`). No pagination for v1.

3. **Should the gateway log `clientInfo` from initialize?**
   - What we know: `InitializeRequestParams.client_info` tells us who connected (e.g., "claude-code v1.0.0").
   - Recommendation: Yes, log it at `info` level. Useful for audit and debugging.

## Sources

### Primary (HIGH confidence)
- [MCP Specification 2025-03-26 - Lifecycle](https://modelcontextprotocol.io/specification/2025-03-26/basic/lifecycle) - Initialize handshake, state machine, version negotiation, capability negotiation
- [MCP Specification 2025-03-26 - Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) - stdio transport rules: newline-delimited, no embedded newlines, stderr for logging, UTF-8
- [MCP Specification 2025-03-26 - Tools](https://modelcontextprotocol.io/specification/2025-03-26/server/tools) - tools/list and tools/call request/response format, Tool schema, error handling
- [rmcp 0.16.0 docs (model module)](https://docs.rs/rmcp/0.16.0/rmcp/model/index.html) - All MCP protocol types
- [rmcp 0.16.0 docs (InitializeResult)](https://docs.rs/rmcp/0.16.0/rmcp/model/struct.InitializeResult.html) - protocol_version, capabilities, server_info, instructions fields
- [rmcp 0.16.0 docs (ServerCapabilities)](https://docs.rs/rmcp/0.16.0/rmcp/model/struct.ServerCapabilities.html) - Builder pattern, enable_tools()
- [rmcp 0.16.0 docs (ProtocolVersion)](https://docs.rs/rmcp/0.16.0/rmcp/model/struct.ProtocolVersion.html) - V_2025_03_26, LATEST constants

### Secondary (MEDIUM confidence)
- [rmcp GitHub Cargo.toml](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/Cargo.toml) - Feature flag analysis, default-features = false gives types only

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - rmcp types verified on docs.rs, MCP spec read directly
- Architecture: HIGH - patterns derived from spec requirements and existing Phase 1 code
- Pitfalls: HIGH - grounded in MCP spec rules and Rust async patterns

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (MCP spec is stable at 2025-03-26; rmcp 0.16 pinned)
