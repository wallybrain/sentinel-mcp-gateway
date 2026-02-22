use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use sentinel_gateway::backend::HttpBackend;
use sentinel_gateway::catalog::create_stub_catalog;
use sentinel_gateway::gateway::run_dispatch;
use sentinel_gateway::protocol::id_remapper::IdRemapper;

async fn spawn_dispatch() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    let catalog = create_stub_catalog();
    let catalog: &'static _ = Box::leak(Box::new(catalog));

    let backends: HashMap<String, HttpBackend> = HashMap::new();
    let backends: &'static _ = Box::leak(Box::new(backends));

    let id_remapper = IdRemapper::new();
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(in_rx, out_tx, catalog, backends, id_remapper).await;
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

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(in_rx, out_tx, catalog, backends, id_remapper).await;
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
