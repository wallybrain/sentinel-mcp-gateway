use std::collections::HashMap;

use rmcp::model::ListToolsResult;
use serde_json::json;
use tokio::sync::mpsc;

use crate::backend::HttpBackend;
use crate::catalog::ToolCatalog;
use crate::protocol::id_remapper::IdRemapper;
use crate::protocol::jsonrpc::{
    JsonRpcId, JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND,
    PARSE_ERROR,
};
use crate::protocol::mcp::{handle_initialize, McpState};

const NOT_INITIALIZED_CODE: i32 = -32002;

/// Central dispatch loop that processes MCP messages.
///
/// Receives JSON-RPC messages from `rx`, routes them through the MCP
/// state machine, dispatches to the appropriate handler, and sends
/// responses through `tx`.
pub async fn run_dispatch(
    mut rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<String>,
    catalog: &ToolCatalog,
    backends: &HashMap<String, HttpBackend>,
    id_remapper: &IdRemapper,
) -> anyhow::Result<()> {
    let mut state = McpState::Created;

    while let Some(line) = rx.recv().await {
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse JSON-RPC request");
                let resp = JsonRpcResponse::error(
                    JsonRpcId::Null,
                    PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                send_response(&tx, &resp).await;
                continue;
            }
        };

        let is_notification = request.is_notification();

        if !state.can_accept_method(&request.method) {
            if !is_notification {
                let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                let resp = JsonRpcResponse::error(
                    id,
                    NOT_INITIALIZED_CODE,
                    "Server not initialized".to_string(),
                );
                send_response(&tx, &resp).await;
            }
            continue;
        }

        match request.method.as_str() {
            "initialize" => {
                let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                let params = request.params.unwrap_or(json!({}));
                match handle_initialize(params) {
                    Ok(result) => {
                        let resp = JsonRpcResponse::success(id, result);
                        send_response(&tx, &resp).await;
                    }
                    Err((code, message)) => {
                        let resp = JsonRpcResponse::error(id, code, message);
                        send_response(&tx, &resp).await;
                    }
                }
                state = McpState::Initializing;
                tracing::info!("MCP state -> Initializing");
            }

            "notifications/initialized" => {
                state = McpState::Operational;
                tracing::info!("MCP state -> Operational");
                // Notification: no response
            }

            "tools/list" => {
                if !is_notification {
                    let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                    let tools = catalog.all_tools();
                    let result = ListToolsResult::with_all_items(tools);
                    let value = serde_json::to_value(&result)
                        .expect("ListToolsResult serialization cannot fail");
                    let resp = JsonRpcResponse::success(id, value);
                    send_response(&tx, &resp).await;
                }
            }

            "tools/call" => {
                if !is_notification {
                    let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                    let resp =
                        handle_tools_call(id, request.params, catalog, backends, id_remapper)
                            .await;
                    send_response(&tx, &resp).await;
                }
            }

            "ping" => {
                if !is_notification {
                    let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                    let resp = JsonRpcResponse::success(id, json!({}));
                    send_response(&tx, &resp).await;
                }
            }

            method => {
                if !is_notification {
                    let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                    let resp = JsonRpcResponse::error(
                        id,
                        METHOD_NOT_FOUND,
                        format!("Method not found: {method}"),
                    );
                    send_response(&tx, &resp).await;
                }
            }
        }
    }

    state = McpState::Closed;
    tracing::info!("MCP state -> Closed (input channel closed)");
    let _ = state; // suppress unused warning
    Ok(())
}

async fn handle_tools_call(
    client_id: JsonRpcId,
    params: Option<serde_json::Value>,
    catalog: &ToolCatalog,
    backends: &HashMap<String, HttpBackend>,
    id_remapper: &IdRemapper,
) -> JsonRpcResponse {
    // Extract tool name from params
    let tool_name = match params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    {
        Some(name) => name.to_string(),
        None => {
            return JsonRpcResponse::error(
                client_id,
                INVALID_PARAMS,
                "Missing tool name in params".to_string(),
            );
        }
    };

    // Route via catalog
    let backend_name = match catalog.route(&tool_name) {
        Some(name) => name.to_string(),
        None => {
            return JsonRpcResponse::error(
                client_id,
                INVALID_PARAMS,
                format!("Unknown tool: {tool_name}"),
            );
        }
    };

    // Get backend
    let backend = match backends.get(&backend_name) {
        Some(b) => b,
        None => {
            tracing::error!(
                backend = %backend_name,
                tool = %tool_name,
                "Backend in catalog but not in backends map"
            );
            return JsonRpcResponse::error(
                client_id,
                INTERNAL_ERROR,
                format!("Backend unavailable: {backend_name}"),
            );
        }
    };

    // Remap ID
    let gateway_id = id_remapper.remap(client_id, &backend_name);

    // Build outbound request
    let outbound = json!({
        "jsonrpc": "2.0",
        "id": gateway_id,
        "method": "tools/call",
        "params": params,
    });
    let body = serde_json::to_string(&outbound).expect("JSON serialization cannot fail");

    // Send to backend
    match backend.send(&body).await {
        Ok(response_str) => {
            // Parse response and restore original ID
            match serde_json::from_str::<JsonRpcResponse>(&response_str) {
                Ok(mut backend_resp) => {
                    if let Some((original_id, _)) = id_remapper.restore(gateway_id) {
                        backend_resp.id = original_id;
                    }
                    backend_resp
                }
                Err(e) => {
                    // Restore ID even on parse failure
                    let original = id_remapper
                        .restore(gateway_id)
                        .map(|(id, _)| id)
                        .unwrap_or(JsonRpcId::Null);
                    tracing::error!(error = %e, "Failed to parse backend response");
                    JsonRpcResponse::error(
                        original,
                        INTERNAL_ERROR,
                        format!("Invalid backend response: {e}"),
                    )
                }
            }
        }
        Err(e) => {
            // Restore ID on error
            let original = id_remapper
                .restore(gateway_id)
                .map(|(id, _)| id)
                .unwrap_or(JsonRpcId::Null);
            tracing::error!(error = %e, backend = %backend_name, "Backend call failed");
            JsonRpcResponse::error(
                original,
                INTERNAL_ERROR,
                format!("Backend error: {e}"),
            )
        }
    }
}

async fn send_response(tx: &mpsc::Sender<String>, resp: &JsonRpcResponse) {
    let serialized = serde_json::to_string(resp).expect("response serialization cannot fail");
    if tx.send(serialized).await.is_err() {
        tracing::warn!("Output channel closed, dropping response");
    }
}
