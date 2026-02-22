# Phase 4: Authentication & Authorization - Research

**Researched:** 2026-02-22
**Domain:** JWT authentication + RBAC authorization in Rust
**Confidence:** HIGH

## Summary

Phase 4 adds JWT token validation and role-based access control to the Sentinel Gateway. The gateway currently accepts all MCP requests without authentication (Phases 1-3 focused on protocol, catalog, and routing). This phase gates every request on a valid JWT and filters/blocks tool access based on the caller's role.

The implementation is straightforward: the `jsonwebtoken` crate (v10.x) handles HS256 JWT validation with claim checking (exp, iss, aud), and the existing `RbacConfig` in `sentinel.toml` already defines the role-to-permission mapping. The main architectural work is threading auth context through the dispatch loop and ensuring the same RBAC check function is used for both `tools/list` filtering and `tools/call` enforcement (success criterion 5).

**Primary recommendation:** Use `jsonwebtoken = { version = "10", default-features = false, features = ["rust_crypto"] }` for JWT validation. Create an `auth` module with a `Claims` struct and `validate_token()` function, then an `rbac` module with a single `is_tool_allowed(role, tool_name, rbac_config) -> bool` function used in both dispatch paths.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AUTH-01 | Gateway validates JWT tokens (HS256) on every incoming request, checking exp/iss/aud/jti claims | `jsonwebtoken` v10 Validation struct supports all these claims natively. DecodingKey::from_secret() for HS256. |
| AUTH-02 | Gateway rejects requests with missing, expired, or malformed tokens with JSON-RPC error response | `jsonwebtoken` ErrorKind enum provides ExpiredSignature, InvalidIssuer, InvalidAudience, InvalidToken, MissingRequiredClaim. Map each to JSON-RPC error code -32001 (custom auth error). |
| AUTH-03 | Gateway extracts role claims from JWT for downstream RBAC decisions | Custom `Claims` struct with `role: String` field. Extracted after decode and threaded through dispatch as `CallerIdentity`. |
| AUTHZ-01 | Gateway enforces per-tool, per-role permissions defined in TOML config | Existing `RbacConfig` with `RoleConfig { permissions, denied_tools }` already parsed from TOML. Build `is_tool_allowed()` against this. |
| AUTHZ-02 | `tools/list` responses are filtered by caller's role | Modify `tools/list` handler to call `catalog.tools_for_role(role, rbac_config)` instead of `catalog.all_tools()`. |
| AUTHZ-03 | `tools/call` requests are rejected if caller's role lacks permission | Add RBAC check before catalog routing in `handle_tools_call()`. Same `is_tool_allowed()` function. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| jsonwebtoken | 10.3 | JWT decode/validate with HS256 | De facto Rust JWT library, 72M+ downloads, actively maintained |

### Feature Selection

Use `rust_crypto` backend (not `aws_lc_rs`):
- `aws_lc_rs` requires C/CMake toolchain for native builds, complicates Docker multi-stage
- `rust_crypto` is pure Rust, compiles everywhere, sufficient for HS256
- Disable `use_pem` default feature (not needed for HMAC symmetric keys)

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| jsonwebtoken | jwt-simple | jwt-simple has nicer ergonomics but lower adoption (4M vs 72M downloads) |
| jsonwebtoken | alcoholic_jwt | Only for JWKs/RS256, not applicable to HS256 |

**Installation:**
```toml
jsonwebtoken = { version = "10", default-features = false, features = ["rust_crypto"] }
```

No other new dependencies needed. `serde` and `serde_json` already in Cargo.toml.

## Architecture Patterns

### New Module Structure
```
src/
├── auth/
│   ├── mod.rs           # pub mod jwt; pub mod rbac;
│   ├── jwt.rs           # Claims struct, validate_token(), JwtValidator
│   └── rbac.rs          # is_tool_allowed(), CallerIdentity
├── gateway.rs           # Modified: thread CallerIdentity through dispatch
├── catalog/mod.rs       # Modified: add tools_for_role() method
└── config/types.rs      # Existing: RbacConfig already defined
```

### Pattern 1: JWT Validator (Reusable, Constructed Once)
**What:** A `JwtValidator` struct holding the `DecodingKey` and `Validation` config, constructed once at startup.
**When to use:** Always -- avoids reconstructing validation rules per request.
**Example:**
```rust
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,           // Client/user identifier
    pub role: String,          // Role for RBAC (e.g., "admin", "developer", "viewer")
    pub iss: String,           // Issuer
    pub aud: String,           // Audience (can be String or Vec<String>)
    pub exp: usize,            // Expiration (Unix timestamp)
    pub iat: Option<usize>,    // Issued at
    pub jti: Option<String>,   // Unique token ID
}

pub struct JwtValidator {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtValidator {
    pub fn new(secret: &[u8], issuer: &str, audience: &str) -> Self {
        let decoding_key = DecodingKey::from_secret(secret);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_exp = true;
        // 60 second leeway is the default, appropriate for clock skew
        Self { decoding_key, validation }
    }

    pub fn validate(&self, token: &str) -> Result<Claims, AuthError> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }
}
```

### Pattern 2: CallerIdentity Threading
**What:** A struct carrying the authenticated caller's identity through the dispatch pipeline.
**When to use:** After JWT validation, before any handler.
**Example:**
```rust
pub struct CallerIdentity {
    pub subject: String,    // From JWT sub claim
    pub role: String,       // From JWT role claim
    pub token_id: Option<String>, // From JWT jti claim (for audit logging later)
}
```

### Pattern 3: Single RBAC Check Function
**What:** One function used by both `tools/list` and `tools/call` to ensure consistent authorization.
**When to use:** Always -- success criterion 5 mandates this.
**Example:**
```rust
pub fn is_tool_allowed(role: &str, tool_name: &str, rbac: &RbacConfig) -> bool {
    let role_config = match rbac.roles.get(role) {
        Some(rc) => rc,
        None => return false, // Unknown role = deny all
    };

    // Check deny list first
    if role_config.denied_tools.contains(&tool_name.to_string()) {
        return false;
    }

    // Check permissions
    if role_config.permissions.contains(&"*".to_string()) {
        return true; // Wildcard = allow all
    }

    // "tools.execute" permission grants tools/call access
    // "tools.read" permission grants tools/list visibility
    // For simplicity: any permission that isn't empty allows the tool
    // unless it's in denied_tools
    !role_config.permissions.is_empty()
}
```

### Pattern 4: Auth in stdio Transport (Where JWT Comes From)
**What:** Since the gateway uses stdio transport (not HTTP), the JWT must arrive as part of the MCP protocol, not HTTP headers.
**When to use:** Critical design decision for this phase.

**Options (ranked by practicality):**
1. **Environment variable at startup** -- single client per stdio session, token set as `SENTINEL_TOKEN` env var. Simplest, matches current single-client stdio architecture.
2. **Initialize params** -- client sends token in `initialize` request params. More MCP-native but non-standard.
3. **Custom notification** -- client sends `sentinel/authenticate` notification with token before any other request. Flexible but custom protocol.

**Recommendation:** Option 1 (env var) for v1. The stdio transport is inherently single-session (one client pipes to stdin). The client passes the token via env var when spawning the gateway. This avoids protocol extensions and is compatible with `claude_desktop_config.json` which supports env vars per MCP server.

For future HTTP transport (Phase 10+), the token would come from the `Authorization: Bearer <token>` header.

### Anti-Patterns to Avoid
- **Separate RBAC checks for list vs call:** Must be the SAME function (success criterion 5). Do not write two different check paths.
- **Checking auth after routing:** Auth must happen BEFORE tool routing to prevent timing side-channels that leak which tools exist.
- **Hardcoding roles:** Roles come from config, not from code constants.
- **Panicking on invalid tokens:** Return JSON-RPC error, never panic.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JWT signature verification | Custom HMAC validation | `jsonwebtoken::decode()` | Cryptographic operations are trivially easy to get wrong |
| Claim validation (exp, nbf, iss, aud) | Manual timestamp comparison | `jsonwebtoken::Validation` | Handles leeway, clock skew, all edge cases |
| Base64 URL decoding of JWT parts | Manual split + decode | `jsonwebtoken` internals | JWT encoding has subtle padding rules |

## Common Pitfalls

### Pitfall 1: Auth Bypass via tools/list
**What goes wrong:** Implementing RBAC on `tools/call` but forgetting `tools/list`, leaking tool names to unauthorized users.
**Why it happens:** `tools/list` feels "read-only" and harmless.
**How to avoid:** Use the SAME `is_tool_allowed()` function in both paths. The known gotcha in STATE.md already flags this: "RBAC must filter both tools/list AND tools/call".
**Warning signs:** Tests that only check `tools/call` authorization.

### Pitfall 2: Token Extraction Location
**What goes wrong:** Looking for JWT in HTTP headers when the transport is stdio.
**Why it happens:** Most JWT guides assume HTTP APIs.
**How to avoid:** For stdio transport, use environment variable. Document clearly that the token source differs by transport.
**Warning signs:** Code importing `hyper` or parsing `Authorization` headers in Phase 4.

### Pitfall 3: Clock Skew Rejection
**What goes wrong:** Tokens rejected as expired due to minor clock differences between client and server.
**Why it happens:** Default leeway of 0 seconds in some configurations.
**How to avoid:** jsonwebtoken v10 defaults to 60 seconds leeway, which is appropriate. Do not set leeway to 0.
**Warning signs:** Intermittent auth failures in production.

### Pitfall 4: Missing Required Claims
**What goes wrong:** Accepting tokens without `role` claim, leading to RBAC bypass (no role = no restrictions checked).
**Why it happens:** `role` is a custom claim, not in the JWT spec. jsonwebtoken won't require it automatically.
**How to avoid:** After decode, explicitly check that `claims.role` is non-empty. Return auth error if missing.
**Warning signs:** Tests that use tokens without role claims.

### Pitfall 5: Wildcard Permission Overmatch
**What goes wrong:** `"*"` in permissions matching more than intended (e.g., matching denied_tools).
**Why it happens:** Unclear precedence between permissions and denied_tools.
**How to avoid:** Deny list takes precedence over wildcard. Check denied_tools BEFORE checking permissions.
**Warning signs:** Admin role being blocked by denied_tools not working.

### Pitfall 6: Breaking Existing Tests
**What goes wrong:** All existing integration tests fail because they don't provide JWT tokens.
**Why it happens:** Auth is now mandatory but tests were written pre-auth.
**How to avoid:** Create a test helper that generates valid JWTs for tests. Update `spawn_dispatch()` to accept auth config and create a "test mode" with a known secret.
**Warning signs:** 60+ test failures after adding auth.

## Code Examples

### JWT Token Generation (for tests and CLI token creation)
```rust
use jsonwebtoken::{encode, EncodingKey, Header};

fn create_test_token(role: &str, secret: &[u8]) -> String {
    let claims = Claims {
        sub: "test-client".to_string(),
        role: role.to_string(),
        iss: "sentinel-gateway".to_string(),
        aud: "sentinel-api".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        iat: Some(chrono::Utc::now().timestamp() as usize),
        jti: Some(uuid::Uuid::new_v4().to_string()),
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret)).unwrap()
}
```

Note: For timestamps without adding `chrono` dependency, use `std::time`:
```rust
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}
```

### Error Mapping (jsonwebtoken errors to JSON-RPC errors)
```rust
use jsonwebtoken::errors::ErrorKind;

// Custom auth error code (server-defined range: -32000 to -32099)
const AUTH_ERROR: i32 = -32001;
const AUTHZ_ERROR: i32 = -32003;

fn map_jwt_error(err: &jsonwebtoken::errors::Error) -> (i32, String) {
    match err.kind() {
        ErrorKind::ExpiredSignature => (AUTH_ERROR, "Token expired".to_string()),
        ErrorKind::InvalidIssuer => (AUTH_ERROR, "Invalid token issuer".to_string()),
        ErrorKind::InvalidAudience => (AUTH_ERROR, "Invalid token audience".to_string()),
        ErrorKind::InvalidSignature => (AUTH_ERROR, "Invalid token signature".to_string()),
        ErrorKind::InvalidToken => (AUTH_ERROR, "Malformed token".to_string()),
        ErrorKind::MissingRequiredClaim(claim) => {
            (AUTH_ERROR, format!("Missing required claim: {claim}"))
        }
        _ => (AUTH_ERROR, "Authentication failed".to_string()),
    }
}
```

### Dispatch Loop Integration Point
```rust
// In gateway.rs run_dispatch(), after parsing request but before method dispatch:
// 1. Extract token (from env var for stdio transport)
// 2. Validate token -> CallerIdentity
// 3. Pass CallerIdentity to handlers

// For tools/list:
let filtered_tools = catalog.all_tools()
    .into_iter()
    .filter(|tool| is_tool_allowed(&caller.role, &tool.name, &rbac_config))
    .collect::<Vec<_>>();

// For tools/call:
if !is_tool_allowed(&caller.role, &tool_name, &rbac_config) {
    return JsonRpcResponse::error(
        client_id,
        AUTHZ_ERROR,
        format!("Permission denied for tool: {tool_name}"),
    );
}
```

### Config Integration (existing RbacConfig)
The TOML config already defines the RBAC structure:
```toml
[rbac.roles.admin]
permissions = ["*"]

[rbac.roles.developer]
permissions = ["tools.read", "tools.execute"]
denied_tools = []

[rbac.roles.viewer]
permissions = ["tools.read"]
```

The existing `RoleConfig` struct has `permissions: Vec<String>` and `denied_tools: Vec<String>`. This is sufficient. The `is_tool_allowed()` function interprets these fields.

**Permission semantics to implement:**
- `"*"` = allow all tools (unless in denied_tools)
- `"tools.read"` = can see tools in tools/list
- `"tools.execute"` = can call tools via tools/call
- `denied_tools` = always denied, overrides permissions

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| jsonwebtoken v8/v9 (ring backend) | jsonwebtoken v10 (pluggable crypto backends) | 2024-2025 | Must select `rust_crypto` or `aws_lc_rs` feature |
| Single `decode()` function | Same API, different backend selection | v10 | No API changes for HS256 usage |

## Open Questions

1. **Should `initialize` and `ping` be exempt from auth?**
   - What we know: MCP spec expects `initialize` to work for handshake. `ping` is a liveness check.
   - What's unclear: Whether auth should gate the entire session (validate once at start) or per-request.
   - Recommendation: Validate token once during session setup (before `initialize` completes). After validation, all requests in the session are authorized. This matches stdio's single-session model. `ping` should be exempt (it's a transport-level concern).

2. **Should unknown roles be denied or given a default?**
   - What we know: A JWT with `role: "intern"` won't match any configured role.
   - Recommendation: Deny by default. Unknown role = no permissions. Log a warning.

3. **Should `run_dispatch` signature change?**
   - What we know: Currently takes `catalog`, `backends`, `id_remapper`. Needs to also take auth config.
   - Recommendation: Add `JwtValidator` and `&RbacConfig` parameters. This changes the function signature and requires updating all callers (main.rs and tests).

## Sources

### Primary (HIGH confidence)
- [jsonwebtoken docs.rs](https://docs.rs/jsonwebtoken/latest/jsonwebtoken/) - Validation struct, DecodingKey API, error types
- [jsonwebtoken GitHub](https://github.com/Keats/jsonwebtoken) - HS256 examples, v10 feature flags
- [jsonwebtoken Validation struct](https://docs.rs/jsonwebtoken/latest/jsonwebtoken/struct.Validation.html) - All validation fields and defaults

### Secondary (MEDIUM confidence)
- [SSOJet JWT validation guide](https://ssojet.com/jwt-validation/validate-jwt-using-hs256-in-rust) - HS256 validation pattern
- [OneUptime Rust JWT guide](https://oneuptime.com/blog/post/2026-01-07-rust-jwt-authentication/view) - Best practices

### Project Sources (HIGH confidence)
- Existing `src/config/types.rs` - RbacConfig, RoleConfig already defined
- Existing `src/gateway.rs` - Dispatch loop integration point
- Existing `sentinel.toml` - RBAC role definitions already in config
- `STATE.md` - Known gotcha: "RBAC must filter both tools/list AND tools/call"

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - jsonwebtoken is the only serious Rust JWT crate, v10 API verified via docs.rs
- Architecture: HIGH - Existing config types and dispatch loop provide clear integration points
- Pitfalls: HIGH - Well-known JWT/RBAC patterns, project-specific gotchas already documented in STATE.md

**Research date:** 2026-02-22
**Valid until:** 2026-04-22 (60 days - JWT/auth patterns are stable)
