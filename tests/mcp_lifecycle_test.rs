use sentinel_gateway::protocol::mcp::{McpState, handle_initialize};

// =============================================================================
// State machine tests
// =============================================================================

#[test]
fn test_created_accepts_initialize() {
    assert!(McpState::Created.can_accept_method("initialize"));
}

#[test]
fn test_created_accepts_ping() {
    assert!(McpState::Created.can_accept_method("ping"));
}

#[test]
fn test_created_rejects_tools_list() {
    assert!(!McpState::Created.can_accept_method("tools/list"));
}

#[test]
fn test_created_rejects_tools_call() {
    assert!(!McpState::Created.can_accept_method("tools/call"));
}

#[test]
fn test_initializing_accepts_initialized_notification() {
    assert!(McpState::Initializing.can_accept_method("notifications/initialized"));
}

#[test]
fn test_initializing_rejects_initialize() {
    assert!(!McpState::Initializing.can_accept_method("initialize"));
}

#[test]
fn test_initializing_accepts_ping() {
    assert!(McpState::Initializing.can_accept_method("ping"));
}

#[test]
fn test_operational_accepts_all() {
    let methods = [
        "initialize",
        "tools/list",
        "tools/call",
        "ping",
        "notifications/initialized",
        "some/arbitrary/method",
    ];
    for method in methods {
        assert!(
            McpState::Operational.can_accept_method(method),
            "Operational should accept '{method}'"
        );
    }
}

#[test]
fn test_closed_rejects_all() {
    let methods = [
        "initialize",
        "tools/list",
        "tools/call",
        "ping",
        "notifications/initialized",
        "some/arbitrary/method",
    ];
    for method in methods {
        assert!(
            !McpState::Closed.can_accept_method(method),
            "Closed should reject '{method}'"
        );
    }
}

// =============================================================================
// Initialize handler tests
// =============================================================================

#[test]
fn test_initialize_returns_valid_response() {
    let params = serde_json::json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": {"name": "test-client", "version": "1.0.0"}
    });

    let result = handle_initialize(params).expect("initialize should succeed");

    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert!(result["capabilities"]["tools"].is_object(), "tools capability must be present");
    assert_eq!(result["serverInfo"]["name"], "sentinel-gateway");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(result["instructions"].is_string(), "instructions must be present");
}

#[test]
fn test_initialize_with_invalid_params() {
    let params = serde_json::json!({});

    let result = handle_initialize(params);

    assert!(result.is_err(), "invalid params should return Err, not panic");
    let (code, msg) = result.unwrap_err();
    assert_eq!(code, -32602, "error code should be INVALID_PARAMS (-32602)");
    assert!(msg.contains("Invalid initialize params"), "error message should describe the issue");
}
