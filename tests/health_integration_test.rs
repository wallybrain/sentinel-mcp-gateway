use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use sentinel_gateway::backend::Backend;
use sentinel_gateway::catalog::create_stub_catalog;
use sentinel_gateway::config::types::{KillSwitchConfig, RateLimitConfig, RbacConfig, RoleConfig};
use sentinel_gateway::gateway::run_dispatch;
use sentinel_gateway::health::circuit_breaker::{CircuitBreaker, CircuitState};
use sentinel_gateway::health::server::{BackendHealth, BackendHealthMap};
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
async fn test_circuit_breaker_blocks_after_failures() {
    // Create circuit breakers with threshold=2 for stub-sqlite backend
    let mut circuit_breakers: HashMap<String, CircuitBreaker> = HashMap::new();
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    circuit_breakers.insert("stub-sqlite".to_string(), cb);

    let catalog = create_stub_catalog();
    let catalog: &'static _ = Box::leak(Box::new(catalog));
    let backends: HashMap<String, Backend> = HashMap::new();
    let backends: &'static _ = Box::leak(Box::new(backends));
    let id_remapper = IdRemapper::new();
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));
    let rbac = default_admin_rbac();
    let rbac: &'static _ = Box::leak(Box::new(rbac));
    let rate_limiter = RateLimiter::new(&RateLimitConfig::default());
    let rate_limiter: &'static _ = Box::leak(Box::new(rate_limiter));
    let kill_switch = KillSwitchConfig::default();
    let kill_switch: &'static _ = Box::leak(Box::new(kill_switch));
    let circuit_breakers: &'static _ = Box::leak(Box::new(circuit_breakers));
    let cancel = CancellationToken::new();

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(
            in_rx, out_tx, catalog, backends, id_remapper, None, rbac, None,
            rate_limiter, kill_switch, circuit_breakers, cancel,
        )
        .await;
    });

    do_handshake(&in_tx, &mut out_rx).await;

    // Call a tool routed to stub-sqlite -- should get CIRCUIT_OPEN_ERROR
    let req = json!({
        "jsonrpc": "2.0", "id": 10, "method": "tools/call",
        "params": {"name": "read_query", "arguments": {"query": "SELECT 1"}}
    })
    .to_string();
    let resp = send_and_recv(&in_tx, &mut out_rx, &req).await;
    let error = resp.get("error").expect("should have error");
    assert_eq!(error["code"], -32007, "should be CIRCUIT_OPEN_ERROR");
    assert!(
        error["message"].as_str().unwrap().contains("circuit open"),
        "message should mention circuit open"
    );
}

#[tokio::test]
async fn test_dispatch_exits_on_cancel() {
    let catalog = create_stub_catalog();
    let catalog: &'static _ = Box::leak(Box::new(catalog));
    let backends: HashMap<String, Backend> = HashMap::new();
    let backends: &'static _ = Box::leak(Box::new(backends));
    let id_remapper = IdRemapper::new();
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));
    let rbac = default_admin_rbac();
    let rbac: &'static _ = Box::leak(Box::new(rbac));
    let rate_limiter = RateLimiter::new(&RateLimitConfig::default());
    let rate_limiter: &'static _ = Box::leak(Box::new(rate_limiter));
    let kill_switch = KillSwitchConfig::default();
    let kill_switch: &'static _ = Box::leak(Box::new(kill_switch));
    let circuit_breakers: HashMap<String, CircuitBreaker> = HashMap::new();
    let circuit_breakers: &'static _ = Box::leak(Box::new(circuit_breakers));

    let cancel = CancellationToken::new();
    let cancel_trigger = cancel.clone();

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, _out_rx) = mpsc::channel::<String>(64);

    let handle = tokio::spawn(async move {
        run_dispatch(
            in_rx, out_tx, catalog, backends, id_remapper, None, rbac, None,
            rate_limiter, kill_switch, circuit_breakers, cancel,
        )
        .await
    });

    // Keep in_tx alive so stdin doesn't close
    let _keep = in_tx;

    // Cancel and verify dispatch exits
    cancel_trigger.cancel();
    let result = timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "dispatch should exit after cancel");
    assert!(result.unwrap().unwrap().is_ok(), "dispatch should return Ok");
}

#[tokio::test]
async fn test_health_endpoint_liveness() {
    let health_map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
    let cancel = CancellationToken::new();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let hm = health_map.clone();
    let cancel_server = cancel.clone();
    tokio::spawn(async move {
        use sentinel_gateway::health::server::build_health_router;
        let app = build_health_router(hm);
        axum::serve(listener, app)
            .with_graceful_shutdown(cancel_server.cancelled_owned())
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    cancel.cancel();
}

#[tokio::test]
async fn test_health_endpoint_readiness_no_backends() {
    let health_map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
    let cancel = CancellationToken::new();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let hm = health_map.clone();
    let cancel_server = cancel.clone();
    tokio::spawn(async move {
        use sentinel_gateway::health::server::build_health_router;
        let app = build_health_router(hm);
        axum::serve(listener, app)
            .with_graceful_shutdown(cancel_server.cancelled_owned())
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 503, "empty health map should return 503");

    cancel.cancel();
}

#[tokio::test]
async fn test_health_endpoint_readiness_with_healthy_backend() {
    let health_map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
    health_map.write().await.insert(
        "test-backend".to_string(),
        BackendHealth {
            healthy: true,
            last_check: std::time::Instant::now(),
            consecutive_failures: 0,
        },
    );

    let cancel = CancellationToken::new();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let hm = health_map.clone();
    let cancel_server = cancel.clone();
    tokio::spawn(async move {
        use sentinel_gateway::health::server::build_health_router;
        let app = build_health_router(hm);
        axum::serve(listener, app)
            .with_graceful_shutdown(cancel_server.cancelled_owned())
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "healthy backend should return 200");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");

    cancel.cancel();
}
