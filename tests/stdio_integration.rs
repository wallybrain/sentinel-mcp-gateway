use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::NamedTempFile;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use sentinel_gateway::backend::stdio::{discover_stdio_tools, kill_process_group};
use sentinel_gateway::backend::{Backend, StdioBackend};
use sentinel_gateway::catalog::ToolCatalog;
use sentinel_gateway::config::types::{
    BackendConfig, BackendType, KillSwitchConfig, RateLimitConfig, RbacConfig, RoleConfig,
};
use sentinel_gateway::gateway::run_dispatch;
use sentinel_gateway::health::circuit_breaker::CircuitBreaker;
use sentinel_gateway::protocol::id_remapper::IdRemapper;
use sentinel_gateway::ratelimit::RateLimiter;

const MOCK_MCP_SERVER: &str = r#"
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except json.JSONDecodeError:
        continue
    method = req.get("method", "")
    req_id = req.get("id")
    if req_id is None:
        continue
    if method == "initialize":
        resp = {"jsonrpc":"2.0","id":req_id,"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"mock-stdio","version":"0.1.0"}}}
    elif method == "tools/list":
        resp = {"jsonrpc":"2.0","id":req_id,"result":{"tools":[{"name":"echo_tool","description":"Echoes input back","inputSchema":{"type":"object","properties":{"message":{"type":"string"}}}}]}}
    elif method == "tools/call":
        args = req.get("params", {}).get("arguments", {})
        msg = args.get("message", "no message")
        resp = {"jsonrpc":"2.0","id":req_id,"result":{"content":[{"type":"text","text": msg}]}}
    else:
        resp = {"jsonrpc":"2.0","id":req_id,"error":{"code":-32601,"message":"Method not found"}}
    print(json.dumps(resp), flush=True)
"#;

fn mock_config(script_path: &str) -> BackendConfig {
    BackendConfig {
        name: "mock-stdio".to_string(),
        backend_type: BackendType::Stdio,
        url: None,
        command: Some("python3".to_string()),
        args: vec![script_path.to_string()],
        env: HashMap::new(),
        timeout_secs: 10,
        retries: 0,
        restart_on_exit: false,
        max_restarts: 0,
        health_interval_secs: 300,
        circuit_breaker_threshold: 5,
        circuit_breaker_recovery_secs: 30,
    }
}

fn write_mock_script() -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(MOCK_MCP_SERVER.as_bytes())
        .expect("write mock script");
    f.flush().expect("flush");
    f
}

#[tokio::test]
async fn test_stdio_tool_discovery_returns_tools() {
    let script = write_mock_script();
    let config = mock_config(script.path().to_str().unwrap());

    let (backend, _stdin_handle, stdout_handle) =
        StdioBackend::spawn(&config).expect("spawn failed");

    let tools = discover_stdio_tools(&backend).await.expect("discovery failed");
    assert_eq!(tools.len(), 1, "should discover exactly one tool");
    assert_eq!(tools[0].name.as_ref(), "echo_tool");

    let pid = backend.pid();
    drop(backend);
    kill_process_group(pid);
    let _ = tokio::time::timeout(Duration::from_secs(2), stdout_handle).await;
}

#[tokio::test]
async fn test_stdio_tools_call_through_dispatch() {
    let script = write_mock_script();
    let config = mock_config(script.path().to_str().unwrap());

    let (backend, _stdin_handle, _stdout_handle) =
        StdioBackend::spawn(&config).expect("spawn failed");

    // Discover tools via MCP handshake
    let tools = discover_stdio_tools(&backend).await.expect("discovery failed");
    assert_eq!(tools.len(), 1);

    // Build catalog and backends map
    let mut catalog = ToolCatalog::new();
    catalog.register_backend("mock-stdio", tools);

    let mut backends_map: HashMap<String, Backend> = HashMap::new();
    backends_map.insert("mock-stdio".to_string(), Backend::Stdio(backend.clone()));

    let id_remapper = IdRemapper::new();
    let mut roles = HashMap::new();
    roles.insert(
        "admin".to_string(),
        RoleConfig {
            permissions: vec!["*".to_string()],
            denied_tools: vec![],
        },
    );
    let rbac = RbacConfig { roles };
    let rate_limiter = RateLimiter::new(&RateLimitConfig::default());
    let kill_switch = KillSwitchConfig::default();
    let circuit_breakers: HashMap<String, CircuitBreaker> = HashMap::new();

    // Leak for 'static lifetime (test pattern from existing tests)
    let catalog: &'static _ = Box::leak(Box::new(catalog));
    let backends_map: &'static _ = Box::leak(Box::new(backends_map));
    let id_remapper: &'static _ = Box::leak(Box::new(id_remapper));
    let rbac: &'static _ = Box::leak(Box::new(rbac));
    let rate_limiter: &'static _ = Box::leak(Box::new(rate_limiter));
    let kill_switch: &'static _ = Box::leak(Box::new(kill_switch));
    let circuit_breakers: &'static _ = Box::leak(Box::new(circuit_breakers));

    let cancel = CancellationToken::new();

    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let _ = run_dispatch(
            in_rx, out_tx, catalog, backends_map, id_remapper, None, rbac, None,
            rate_limiter, kill_switch, circuit_breakers, cancel,
        )
        .await;
    });

    // MCP handshake
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}});
    in_tx.send(serde_json::to_string(&init).unwrap()).await.unwrap();
    let resp: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed"),
    )
    .unwrap();
    assert!(resp["result"]["serverInfo"].is_object());

    let notif = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    in_tx.send(serde_json::to_string(&notif).unwrap()).await.unwrap();

    // Verify tools/list includes our stdio tool
    let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    in_tx.send(serde_json::to_string(&list).unwrap()).await.unwrap();
    let resp: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed"),
    )
    .unwrap();
    let tool_names: Vec<&str> = resp["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        tool_names.contains(&"echo_tool"),
        "tools/list should contain echo_tool, got: {:?}",
        tool_names
    );

    // Call the stdio tool through dispatch
    let call = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "echo_tool",
            "arguments": {"message": "hello from dispatch"}
        }
    });
    in_tx.send(serde_json::to_string(&call).unwrap()).await.unwrap();
    let resp: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed"),
    )
    .unwrap();

    assert!(resp["error"].is_null(), "should not have error: {:?}", resp);
    let content = &resp["result"]["content"][0];
    assert_eq!(content["type"], "text");
    assert_eq!(content["text"], "hello from dispatch");

    // Clean up
    let pid = backend.pid();
    kill_process_group(pid);
}

#[tokio::test]
async fn test_kill_process_group_terminates_stdio_child() {
    let script = write_mock_script();
    let config = mock_config(script.path().to_str().unwrap());

    let (backend, _stdin_handle, stdout_handle) =
        StdioBackend::spawn(&config).expect("spawn failed");

    let pid = backend.pid();
    assert!(pid > 0, "should have valid PID");

    kill_process_group(pid);

    // stdout_handle should complete (EOF) once child dies
    let result = tokio::time::timeout(Duration::from_secs(5), stdout_handle).await;
    assert!(result.is_ok(), "stdout reader should exit after process group kill");
}
