use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use sentinel_gateway::catalog::create_stub_catalog;
use sentinel_gateway::gateway::run_dispatch;

async fn spawn_dispatch() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    let catalog = create_stub_catalog();
    // Leak the catalog so it lives for the duration of the dispatch task.
    // In tests this is fine -- the process exits after the test.
    let catalog: &'static _ = Box::leak(Box::new(catalog));

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(in_rx, out_tx, catalog).await;
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
    // Send initialized notification (no response expected)
    tx.send(initialized_notification()).await.unwrap();
    // Brief pause for state transition
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

    // Send initialized notification in Created state (rejected, but no response since notification)
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
