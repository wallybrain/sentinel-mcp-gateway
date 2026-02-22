use rmcp::model::ListToolsResult;
use serde_json::json;
use tokio::sync::mpsc;

use crate::catalog::ToolCatalog;
use crate::protocol::jsonrpc::{
    JsonRpcId, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND, PARSE_ERROR,
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

async fn send_response(tx: &mpsc::Sender<String>, resp: &JsonRpcResponse) {
    let serialized = serde_json::to_string(resp).expect("response serialization cannot fail");
    if tx.send(serialized).await.is_err() {
        tracing::warn!("Output channel closed, dropping response");
    }
}
