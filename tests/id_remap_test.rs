use sentinel_gateway::protocol::jsonrpc::{
    JsonRpcId, JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, PARSE_ERROR,
};
use sentinel_gateway::protocol::id_remapper::IdRemapper;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;

// --- JSON-RPC type tests ---

#[test]
fn deserialize_request_with_number_id() {
    let json = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":null}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.id, Some(JsonRpcId::Number(1)));
    assert_eq!(req.method, "test");
}

#[test]
fn deserialize_request_with_string_id() {
    let json = r#"{"jsonrpc":"2.0","id":"abc-123","method":"test"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.id, Some(JsonRpcId::String("abc-123".to_string())));
}

#[test]
fn deserialize_notification_has_no_id() {
    let json = r#"{"jsonrpc":"2.0","method":"notifications/test"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert!(req.id.is_none());
    assert!(req.is_notification());
}

#[test]
fn serialize_response_with_result() {
    let resp = JsonRpcResponse::success(
        JsonRpcId::Number(1),
        serde_json::json!({"status": "ok"}),
    );
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"result\""));
    assert!(!json.contains("\"error\""));
}

#[test]
fn serialize_response_with_error() {
    let resp = JsonRpcResponse::error(
        JsonRpcId::Number(1),
        INTERNAL_ERROR,
        "something broke".to_string(),
    );
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"error\""));
    assert!(!json.contains("\"result\""));
}

#[test]
fn error_codes_match_spec() {
    assert_eq!(PARSE_ERROR, -32700);
    assert_eq!(INVALID_REQUEST, -32600);
    assert_eq!(METHOD_NOT_FOUND, -32601);
    assert_eq!(INVALID_PARAMS, -32602);
    assert_eq!(INTERNAL_ERROR, -32603);
}

// --- ID remapper tests ---

#[test]
fn remap_produces_unique_ids() {
    let remapper = IdRemapper::new();
    let id = JsonRpcId::Number(42);
    let gw1 = remapper.remap(id.clone(), "backend-a");
    let gw2 = remapper.remap(id, "backend-b");
    assert_ne!(gw1, gw2);
}

#[test]
fn restore_returns_original_id() {
    let remapper = IdRemapper::new();
    let original = JsonRpcId::String("client-req-1".to_string());
    let gw_id = remapper.remap(original.clone(), "my-backend");
    let (restored_id, backend) = remapper.restore(gw_id).unwrap();
    assert_eq!(restored_id, original);
    assert_eq!(backend, "my-backend");
}

#[test]
fn restore_removes_mapping() {
    let remapper = IdRemapper::new();
    let id = JsonRpcId::Number(1);
    let gw_id = remapper.remap(id, "backend");
    assert_eq!(remapper.pending_count(), 1);
    let _ = remapper.restore(gw_id);
    assert_eq!(remapper.pending_count(), 0);
    assert!(remapper.restore(gw_id).is_none());
}

#[test]
fn concurrent_remapping_no_collision() {
    let remapper = Arc::new(IdRemapper::new());
    let mut handles = vec![];

    for thread_idx in 0..10 {
        let r = Arc::clone(&remapper);
        handles.push(thread::spawn(move || {
            let mut ids = Vec::with_capacity(100);
            for req_idx in 0..100u64 {
                let id = JsonRpcId::Number(req_idx);
                let gw_id = r.remap(id, &format!("backend-{thread_idx}"));
                ids.push(gw_id);
            }
            ids
        }));
    }

    let mut all_ids = HashSet::new();
    for handle in handles {
        for id in handle.join().unwrap() {
            assert!(all_ids.insert(id), "duplicate gateway ID detected");
        }
    }
    assert_eq!(all_ids.len(), 1000);
}

#[test]
fn counter_starts_at_one() {
    let remapper = IdRemapper::new();
    let gw_id = remapper.remap(JsonRpcId::Number(0), "backend");
    assert_eq!(gw_id, 1);
}
