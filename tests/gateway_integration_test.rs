use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use sentinel_gateway::auth::jwt::CallerIdentity;
use sentinel_gateway::backend::HttpBackend;
use sentinel_gateway::catalog::create_stub_catalog;
use sentinel_gateway::config::types::{KillSwitchConfig, RateLimitConfig, RbacConfig, RoleConfig};
use sentinel_gateway::gateway::run_dispatch;
use sentinel_gateway::protocol::id_remapper::IdRemapper;
use sentinel_gateway::ratelimit::RateLimiter;

fn default_admin_rbac() -> RbacConfig {
    let mut roles = HashMap::new();
    roles.insert(
        "admin".to_string(),
        RoleConfig {
            permissions: vec!["*".to_string()],
            denied_tools: vec![],
        },
    );
    RbacConfig { roles }
}

fn make_test_rbac() -> RbacConfig {
    let mut roles = HashMap::new();
    roles.insert(
        "admin".to_string(),
        RoleConfig {
            permissions: vec!["*".to_string()],
            denied_tools: vec![],
        },
    );
    roles.insert(
        "developer".to_string(),
        RoleConfig {
            permissions: vec!["tools.read".to_string(), "tools.execute".to_string()],
            denied_tools: vec!["write_query".to_string()],
        },
    );
    roles.insert(
        "viewer".to_string(),
        RoleConfig {
            permissions: vec!["tools.read".to_string()],
            denied_tools: vec![],
        },
    );
    roles.insert(
        "admin_restricted".to_string(),
        RoleConfig {
            permissions: vec!["*".to_string()],
            denied_tools: vec!["execute_workflow".to_string()],
        },
    );
    RbacConfig { roles }
}

fn make_caller(role: &str) -> CallerIdentity {
    CallerIdentity {
        subject: format!("test-{role}"),
        role: role.to_string(),
        token_id: None,
    }
}

/// Spawn dispatch with default admin (no auth) -- existing tests use this.
async fn spawn_dispatch() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    let rbac = default_admin_rbac();
    let rbac: &'static _ = Box::leak(Box::new(rbac));
    spawn_dispatch_with_caller(None, rbac).await
}

/// Spawn dispatch with a specific caller and RBAC config.
async fn spawn_dispatch_with_caller(
    caller: Option<CallerIdentity>,
    rbac: &'static RbacConfig,
) -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig::default())));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig::default()));
    spawn_dispatch_with_config(caller, rbac, rate_limiter, kill_switch).await
}

/// Spawn dispatch with full config control (caller, RBAC, rate limiter, kill switch).
async fn spawn_dispatch_with_config(
    caller: Option<CallerIdentity>,
    rbac: &'static RbacConfig,
    rate_limiter: &'static RateLimiter,
    kill_switch: &'static KillSwitchConfig,
) -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    let catalog = create_stub_catalog();
    let catalog: &'static _ = Box::leak(Box::new(catalog));

    let backends: HashMap<String, HttpBackend> = HashMap::new();
    let backends: &'static _ = Box::leak(Box::new(backends));

    let id_remapper = IdRemapper::new();
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(
            in_rx, out_tx, catalog, backends, id_remapper, caller, rbac, None,
            rate_limiter, kill_switch,
        )
        .await;
    });

    (in_tx, out_rx)
}

async fn send_and_recv(
    tx: &mpsc::Sender<String>,
    rx: &mut mpsc::Receiver<String>,
    msg: &str,
) -> Value {
    tx.send(msg.to_string()).await.unwrap();
    let raw = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("channel closed unexpectedly");
    serde_json::from_str(&raw).expect("invalid JSON response")
}

fn initialize_request() -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    })
    .to_string()
}

fn initialized_notification() -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
    .to_string()
}

async fn do_handshake(tx: &mpsc::Sender<String>, rx: &mut mpsc::Receiver<String>) {
    let resp = send_and_recv(tx, rx, &initialize_request()).await;
    assert!(resp.get("result").is_some(), "initialize should succeed");
    tx.send(initialized_notification()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn test_full_mcp_session() {
    let (tx, mut rx) = spawn_dispatch().await;

    // 1. Initialize
    let resp = send_and_recv(&tx, &mut rx, &initialize_request()).await;
    let result = resp.get("result").expect("should have result");
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert!(result["capabilities"]["tools"].is_object());

    // 2. Initialized notification -> no response
    tx.send(initialized_notification()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        rx.try_recv().is_err(),
        "notification should not produce a response"
    );

    // 3. tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &list_req).await;
    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be array");
    assert_eq!(tools.len(), 4);

    // 4. Ping
    let ping_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "ping"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &ping_req).await;
    assert!(resp.get("result").is_some());
}

#[tokio::test]
async fn test_request_before_initialize_returns_error() {
    let (tx, mut rx) = spawn_dispatch().await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32002);
}

#[tokio::test]
async fn test_parse_error_returns_error() {
    let (tx, mut rx) = spawn_dispatch().await;

    let resp = send_and_recv(&tx, &mut rx, "not json at all").await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32700);
}

#[tokio::test]
async fn test_unknown_method_returns_error() {
    let (tx, mut rx) = spawn_dispatch().await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "foo/bar"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32601);
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("foo/bar"));
}

#[tokio::test]
async fn test_notification_gets_no_response() {
    let (tx, mut rx) = spawn_dispatch().await;

    tx.send(initialized_notification()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        rx.try_recv().is_err(),
        "rejected notification should not produce a response"
    );
}

#[tokio::test]
async fn test_ping_works_before_initialize() {
    let (tx, mut rx) = spawn_dispatch().await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "ping"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    assert!(resp.get("result").is_some());
    assert_eq!(resp["result"], json!({}));
}

// --- New tests for tools/call dispatch ---

#[tokio::test]
async fn test_tools_call_unknown_tool() {
    let (tx, mut rx) = spawn_dispatch().await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "nonexistent",
            "arguments": {}
        }
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32602, "should be INVALID_PARAMS");
    assert!(
        error["message"].as_str().unwrap().contains("Unknown tool"),
        "message should mention unknown tool"
    );
}

#[tokio::test]
async fn test_tools_call_missing_name() {
    let (tx, mut rx) = spawn_dispatch().await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "arguments": {"query": "SELECT 1"}
        }
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32602, "should be INVALID_PARAMS");
    assert!(
        error["message"].as_str().unwrap().contains("Missing tool name"),
        "message should mention missing name"
    );
}

#[tokio::test]
async fn test_tools_call_no_params() {
    let (tx, mut rx) = spawn_dispatch().await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call"
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32602, "should be INVALID_PARAMS for missing params");
}

#[tokio::test]
async fn test_tools_call_routes_to_correct_backend() {
    // Verify catalog routing works correctly for stub backends
    let catalog = create_stub_catalog();

    // Stub catalog has: list_workflows, execute_workflow -> stub-n8n
    //                   read_query, write_query -> stub-sqlite
    assert_eq!(catalog.route("list_workflows"), Some("stub-n8n"));
    assert_eq!(catalog.route("execute_workflow"), Some("stub-n8n"));
    assert_eq!(catalog.route("read_query"), Some("stub-sqlite"));
    assert_eq!(catalog.route("write_query"), Some("stub-sqlite"));
    assert_eq!(catalog.route("nonexistent"), None);
}

#[tokio::test]
async fn test_tools_call_backend_not_in_map_returns_internal_error() {
    // A tool is in the catalog but the backend is NOT in the backends map.
    // This tests the "backend in catalog but not in HashMap" error path.
    let catalog = create_stub_catalog();
    let catalog: &'static _ = Box::leak(Box::new(catalog));

    // Empty backends map (no stub-n8n or stub-sqlite)
    let backends: HashMap<String, HttpBackend> = HashMap::new();
    let backends: &'static _ = Box::leak(Box::new(backends));

    let id_remapper = IdRemapper::new();
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));

    let rbac = default_admin_rbac();
    let rbac: &'static _ = Box::leak(Box::new(rbac));

    let rate_limiter = RateLimiter::new(&RateLimitConfig::default());
    let rate_limiter: &'static _ = Box::leak(Box::new(rate_limiter));
    let kill_switch = KillSwitchConfig::default();
    let kill_switch: &'static _ = Box::leak(Box::new(kill_switch));

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(
            in_rx, out_tx, catalog, backends, id_remapper, None, rbac, None,
            rate_limiter, kill_switch,
        )
        .await;
    });

    // Handshake
    let resp = send_and_recv(&in_tx, &mut out_rx, &initialize_request()).await;
    assert!(resp.get("result").is_some());
    in_tx.send(initialized_notification()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Call a tool that exists in catalog but has no backend
    let req = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {
            "name": "read_query",
            "arguments": {"query": "SELECT 1"}
        }
    })
    .to_string();
    let resp = send_and_recv(&in_tx, &mut out_rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32603, "should be INTERNAL_ERROR");
    assert!(
        error["message"].as_str().unwrap().contains("Backend unavailable"),
        "message should mention backend unavailable"
    );
}

#[tokio::test]
async fn test_id_remapper_round_trip() {
    // Verify IdRemapper correctly remaps and restores IDs
    use sentinel_gateway::protocol::jsonrpc::JsonRpcId;

    let remapper = IdRemapper::new();

    let original_id = JsonRpcId::String("client-req-1".to_string());
    let gateway_id = remapper.remap(original_id.clone(), "backend-a");
    assert!(gateway_id >= 1, "gateway ID should start at 1");

    let restored = remapper.restore(gateway_id);
    assert!(restored.is_some(), "should restore original ID");
    let (id, backend) = restored.unwrap();
    assert_eq!(id, original_id);
    assert_eq!(backend, "backend-a");

    // Second restore should return None (already consumed)
    assert!(remapper.restore(gateway_id).is_none());
}

// --- RBAC integration tests ---

#[tokio::test]
async fn test_viewer_sees_all_tools_in_list() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("viewer");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 4, "viewer with tools.read sees all 4 tools");
}

#[tokio::test]
async fn test_viewer_cannot_call_tools() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("viewer");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32003, "viewer cannot call tools");
    assert!(error["message"].as_str().unwrap().contains("Permission denied"));
}

#[tokio::test]
async fn test_developer_denied_tool_hidden_in_list() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("developer");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"write_query"), "denied tool hidden from list");
    assert_eq!(tools.len(), 3, "developer sees 3 tools (write_query denied)");
}

#[tokio::test]
async fn test_developer_denied_tool_blocked_in_call() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("developer");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "write_query", "arguments": {"query": "DROP TABLE x"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32003, "denied tool returns -32003");
}

#[tokio::test]
async fn test_developer_can_call_allowed_tool() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("developer");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    // Should NOT be -32003 (passes RBAC, then hits backend error since no real backend)
    let error = resp.get("error");
    if let Some(err) = error {
        assert_ne!(err["code"], -32003, "allowed tool should not get AUTHZ error");
    }
}

#[tokio::test]
async fn test_unknown_role_sees_no_tools() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("intern");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 0, "unknown role sees empty catalog");
}

#[tokio::test]
async fn test_unknown_role_cannot_call() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("intern");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32003, "unknown role gets -32003");
}

#[tokio::test]
async fn test_admin_wildcard_sees_all_tools() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("admin");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 4, "admin with wildcard sees all 4 tools");
}

#[tokio::test]
async fn test_admin_denied_tool_override() {
    let rbac: &'static _ = Box::leak(Box::new(make_test_rbac()));
    let caller = make_caller("admin_restricted");
    let (tx, mut rx) = spawn_dispatch_with_caller(Some(caller), rbac).await;
    do_handshake(&tx, &mut rx).await;

    // Check tools/list excludes denied tool
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"execute_workflow"), "denied tool hidden even for admin");

    // Check tools/call blocked
    let req = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "execute_workflow", "arguments": {}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32003, "denied tool returns -32003 even for admin");
}

// --- Audit integration test ---

#[tokio::test]
async fn test_dispatch_accepts_none_audit_tx() {
    // Smoke test: dispatch works correctly with audit_tx: None
    let (tx, mut rx) = spawn_dispatch().await;
    do_handshake(&tx, &mut rx).await;

    // tools/call should work (hits backend error, not audit error)
    let req = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    // Should get INTERNAL_ERROR (no real backend), NOT a panic or audit-related error
    let error = resp.get("error").expect("should have error");
    assert_eq!(
        error["code"], -32603,
        "tools/call with None audit_tx should still work (backend unavailable)"
    );
}

// --- Kill switch integration tests ---

#[tokio::test]
async fn test_kill_switch_tool_disabled_returns_error() {
    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig::default())));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig {
        disabled_tools: vec!["read_query".to_string()],
        disabled_backends: vec![],
    }));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32005, "disabled tool returns KILL_SWITCH_ERROR");
    assert!(
        error["message"].as_str().unwrap().contains("disabled"),
        "message should mention disabled"
    );
}

#[tokio::test]
async fn test_kill_switch_tool_hidden_from_list() {
    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig::default())));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig {
        disabled_tools: vec!["read_query".to_string()],
        disabled_backends: vec![],
    }));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"read_query"), "disabled tool hidden from list");
    assert!(names.contains(&"write_query"), "other tools still present");
    assert_eq!(tools.len(), 3, "3 tools visible (read_query hidden)");
}

#[tokio::test]
async fn test_kill_switch_backend_disabled_returns_error() {
    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig::default())));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig {
        disabled_tools: vec![],
        disabled_backends: vec!["stub-sqlite".to_string()],
    }));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32005, "disabled backend returns KILL_SWITCH_ERROR");
    assert!(
        error["message"].as_str().unwrap().contains("disabled"),
        "message should mention disabled"
    );
}

#[tokio::test]
async fn test_kill_switch_backend_disabled_hides_tools_from_list() {
    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig::default())));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig {
        disabled_tools: vec![],
        disabled_backends: vec!["stub-sqlite".to_string()],
    }));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"read_query"), "sqlite tool hidden");
    assert!(!names.contains(&"write_query"), "sqlite tool hidden");
    assert!(names.contains(&"list_workflows"), "n8n tool still visible");
    assert!(names.contains(&"execute_workflow"), "n8n tool still visible");
    assert_eq!(tools.len(), 2, "only n8n tools visible");
}

// --- Rate limit integration tests ---

#[tokio::test]
async fn test_rate_limit_exceeded_returns_error() {
    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig {
        default_rpm: 2,
        per_tool: HashMap::new(),
    })));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig::default()));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    // First two calls should NOT return rate limit error (they hit backend error instead)
    for i in 1..=2 {
        let req = json!({
            "jsonrpc": "2.0", "id": i, "method": "tools/call",
            "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
        })
        .to_string();
        let resp = send_and_recv(&tx, &mut rx, &req).await;
        let error = resp.get("error");
        if let Some(err) = error {
            assert_ne!(
                err["code"], -32004,
                "call {i} should not be rate limited"
            );
        }
    }

    // Third call should be rate limited
    let req = json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32004, "third call returns RATE_LIMIT_ERROR");
    let retry_after = error["data"]["retryAfter"].as_u64().expect("retryAfter should be integer");
    assert!(retry_after > 0, "retryAfter should be positive");
}

#[tokio::test]
async fn test_rate_limit_per_tool_override() {
    let mut per_tool = HashMap::new();
    per_tool.insert("read_query".to_string(), 1u32);

    let rbac: &'static _ = Box::leak(Box::new(default_admin_rbac()));
    let rate_limiter: &'static _ = Box::leak(Box::new(RateLimiter::new(&RateLimitConfig {
        default_rpm: 100,
        per_tool,
    })));
    let kill_switch: &'static _ = Box::leak(Box::new(KillSwitchConfig::default()));

    let (tx, mut rx) = spawn_dispatch_with_config(None, rbac, rate_limiter, kill_switch).await;
    do_handshake(&tx, &mut rx).await;

    // First call to read_query passes (hits backend error)
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    if let Some(err) = resp.get("error") {
        assert_ne!(err["code"], -32004, "first call should not be rate limited");
    }

    // Second call to read_query is rate limited
    let req = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32004, "second read_query call is rate limited");

    // Call to list_workflows still works (uses default 100 RPM)
    let req = json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "list_workflows", "arguments": {}}
    })
    .to_string();
    let resp = send_and_recv(&tx, &mut rx, &req).await;
    if let Some(err) = resp.get("error") {
        assert_ne!(
            err["code"], -32004,
            "list_workflows should not be rate limited (default 100 RPM)"
        );
    }
}
