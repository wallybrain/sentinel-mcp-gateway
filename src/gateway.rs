use std::collections::HashMap;
use std::sync::Arc;

use rmcp::model::ListToolsResult;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::audit::db::AuditEntry;
use crate::auth::jwt::CallerIdentity;
use crate::auth::rbac::{is_tool_allowed, Permission};
use crate::backend::Backend;
use crate::catalog::ToolCatalog;
use crate::config::hot::SharedHotConfig;
use crate::config::types::RbacConfig;
use crate::health::circuit_breaker::CircuitBreaker;
use crate::metrics::Metrics;
use crate::protocol::id_remapper::IdRemapper;
use crate::protocol::jsonrpc::{
    JsonRpcId, JsonRpcRequest, JsonRpcResponse, CIRCUIT_OPEN_ERROR, INTERNAL_ERROR, INVALID_PARAMS,
    KILL_SWITCH_ERROR, METHOD_NOT_FOUND, PARSE_ERROR, RATE_LIMIT_ERROR,
};
use crate::protocol::mcp::{handle_initialize, McpState};
use crate::validation::SchemaCache;

const NOT_INITIALIZED_CODE: i32 = -32002;
const AUTHZ_ERROR: i32 = -32003;

/// Central dispatch loop that processes MCP messages.
///
/// Receives JSON-RPC messages from `rx`, routes them through the MCP
/// state machine, dispatches to the appropriate handler, and sends
/// responses through `tx`.
pub async fn run_dispatch(
    mut rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<String>,
    catalog: &ToolCatalog,
    backends: &HashMap<String, Backend>,
    id_remapper: &IdRemapper,
    caller: Option<CallerIdentity>,
    rbac_config: &RbacConfig,
    audit_tx: Option<mpsc::Sender<AuditEntry>>,
    hot_config: SharedHotConfig,
    metrics: Option<Arc<Metrics>>,
    schema_cache: &SchemaCache,
    circuit_breakers: &HashMap<String, CircuitBreaker>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let caller = caller.unwrap_or_else(|| CallerIdentity {
        subject: "admin".to_string(),
        role: "admin".to_string(),
        token_id: None,
    });

    let mut state = McpState::Created;

    loop {
        let line = tokio::select! {
            line = rx.recv() => match line {
                Some(l) => l,
                None => break,
            },
            _ = cancel.cancelled() => {
                tracing::info!("Dispatch loop cancelled by shutdown signal");
                break;
            }
        };

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
                    let hc = hot_config.read().await;
                    let tools: Vec<_> = catalog
                        .all_tools()
                        .into_iter()
                        .filter(|tool| {
                            // Kill switch: hide disabled tools
                            if hc.kill_switch.disabled_tools.contains(&tool.name.to_string()) {
                                return false;
                            }
                            // Kill switch: hide tools from disabled backends
                            if let Some(backend) = catalog.route(&tool.name) {
                                if hc.kill_switch.disabled_backends.contains(&backend.to_string()) {
                                    return false;
                                }
                            }
                            // RBAC filter
                            is_tool_allowed(
                                &caller.role,
                                &tool.name,
                                Permission::Read,
                                rbac_config,
                            )
                        })
                        .collect();
                    drop(hc);
                    let result = ListToolsResult::with_all_items(tools);
                    let value = serde_json::to_value(&result)
                        .expect("ListToolsResult serialization cannot fail");
                    let resp = JsonRpcResponse::success(id, value);
                    send_response(&tx, &resp).await;
                }
            }

            "tools/call" => {
                if !is_notification {
                    let request_id = Uuid::new_v4();
                    let start = std::time::Instant::now();
                    let id = request.id.clone().unwrap_or(JsonRpcId::Null);
                    let tool_name = request
                        .params
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str());
                    if let Some(name) = tool_name {
                        // 1. Kill switch check (read from hot config)
                        {
                            let hc = hot_config.read().await;

                            // Kill switch: tool disabled
                            if hc.kill_switch.disabled_tools.contains(&name.to_string()) {
                                if let Some(ref m) = metrics {
                                    m.record_request(name, "killed", 0.0);
                                }
                                let resp = JsonRpcResponse::error(
                                    id.clone(),
                                    KILL_SWITCH_ERROR,
                                    format!("Tool is disabled: {name}"),
                                );
                                send_response(&tx, &resp).await;

                                if let Some(ref atx) = audit_tx {
                                    let entry = AuditEntry {
                                        request_id,
                                        timestamp: chrono::Utc::now(),
                                        client_subject: caller.subject.clone(),
                                        client_role: caller.role.clone(),
                                        tool_name: name.to_string(),
                                        backend_name: catalog.route(name).unwrap_or("unknown").to_string(),
                                        request_args: request.params.clone(),
                                        response_status: "killed".to_string(),
                                        error_message: Some(format!("Tool is disabled: {name}")),
                                        latency_ms: 0,
                                    };
                                    if let Err(e) = atx.try_send(entry) {
                                        tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                    }
                                }

                                continue;
                            }

                            // Kill switch: backend disabled
                            if let Some(backend_name) = catalog.route(name) {
                                if hc.kill_switch.disabled_backends.contains(&backend_name.to_string()) {
                                    if let Some(ref m) = metrics {
                                        m.record_request(name, "killed", 0.0);
                                    }
                                    let resp = JsonRpcResponse::error(
                                        id.clone(),
                                        KILL_SWITCH_ERROR,
                                        format!("Backend is disabled: {backend_name}"),
                                    );
                                    send_response(&tx, &resp).await;

                                    if let Some(ref atx) = audit_tx {
                                        let entry = AuditEntry {
                                            request_id,
                                            timestamp: chrono::Utc::now(),
                                            client_subject: caller.subject.clone(),
                                            client_role: caller.role.clone(),
                                            tool_name: name.to_string(),
                                            backend_name: backend_name.to_string(),
                                            request_args: request.params.clone(),
                                            response_status: "killed".to_string(),
                                            error_message: Some(format!("Backend is disabled: {backend_name}")),
                                            latency_ms: 0,
                                        };
                                        if let Err(e) = atx.try_send(entry) {
                                            tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                        }
                                    }

                                    continue;
                                }
                            }

                            // 2. Rate limit check
                            if let Err(retry_after) = hc.rate_limiter.check(&caller.subject, name) {
                                if let Some(ref m) = metrics {
                                    m.record_request(name, "rate_limited", 0.0);
                                    m.record_rate_limit_hit(name);
                                }
                                let resp = JsonRpcResponse::error_with_data(
                                    id.clone(),
                                    RATE_LIMIT_ERROR,
                                    format!("Rate limit exceeded for tool: {name}"),
                                    json!({"retryAfter": retry_after.ceil() as u64}),
                                );
                                send_response(&tx, &resp).await;

                                if let Some(ref atx) = audit_tx {
                                    let entry = AuditEntry {
                                        request_id,
                                        timestamp: chrono::Utc::now(),
                                        client_subject: caller.subject.clone(),
                                        client_role: caller.role.clone(),
                                        tool_name: name.to_string(),
                                        backend_name: catalog.route(name).unwrap_or("unknown").to_string(),
                                        request_args: request.params.clone(),
                                        response_status: "rate_limited".to_string(),
                                        error_message: Some(format!("Rate limit exceeded for tool: {name}")),
                                        latency_ms: 0,
                                    };
                                    if let Err(e) = atx.try_send(entry) {
                                        tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                    }
                                }

                                continue;
                            }
                        } // drop hc read guard

                        // 3. RBAC check (not hot-reloadable)
                        if !is_tool_allowed(
                            &caller.role,
                            name,
                            Permission::Execute,
                            rbac_config,
                        ) {
                            if let Some(ref m) = metrics {
                                m.record_request(name, "denied", 0.0);
                            }
                            let resp = JsonRpcResponse::error(
                                id,
                                AUTHZ_ERROR,
                                format!("Permission denied for tool: {name}"),
                            );
                            send_response(&tx, &resp).await;

                            if let Some(ref atx) = audit_tx {
                                let entry = AuditEntry {
                                    request_id,
                                    timestamp: chrono::Utc::now(),
                                    client_subject: caller.subject.clone(),
                                    client_role: caller.role.clone(),
                                    tool_name: name.to_string(),
                                    backend_name: catalog.route(name).unwrap_or("unknown").to_string(),
                                    request_args: request.params.clone(),
                                    response_status: "denied".to_string(),
                                    error_message: Some(format!("Permission denied for tool: {name}")),
                                    latency_ms: 0,
                                };
                                if let Err(e) = atx.try_send(entry) {
                                    tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                }
                            }

                            continue;
                        }

                        // 4. Schema validation (after RBAC, before circuit breaker)
                        if let Some(arguments) = request.params.as_ref().and_then(|p| p.get("arguments")) {
                            if let Err(errors) = schema_cache.validate(name, arguments) {
                                let error_msg = format!(
                                    "Invalid arguments for tool {name}: {}",
                                    errors.join("; ")
                                );
                                if let Some(ref m) = metrics {
                                    m.record_request(name, "invalid_args", 0.0);
                                }
                                let resp = JsonRpcResponse::error(
                                    id.clone(),
                                    INVALID_PARAMS,
                                    error_msg.clone(),
                                );
                                send_response(&tx, &resp).await;

                                if let Some(ref atx) = audit_tx {
                                    let entry = AuditEntry {
                                        request_id,
                                        timestamp: chrono::Utc::now(),
                                        client_subject: caller.subject.clone(),
                                        client_role: caller.role.clone(),
                                        tool_name: name.to_string(),
                                        backend_name: catalog.route(name).unwrap_or("unknown").to_string(),
                                        request_args: request.params.clone(),
                                        response_status: "invalid_args".to_string(),
                                        error_message: Some(error_msg),
                                        latency_ms: 0,
                                    };
                                    if let Err(e) = atx.try_send(entry) {
                                        tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                    }
                                }

                                continue;
                            }
                        }

                        // 5. Circuit breaker check
                        if let Some(backend_name) = catalog.route(name) {
                            if let Some(cb) = circuit_breakers.get(backend_name) {
                                if !cb.allow_request() {
                                    if let Some(ref m) = metrics {
                                        m.record_request(name, "circuit_open", 0.0);
                                    }
                                    let resp = JsonRpcResponse::error(
                                        id.clone(),
                                        CIRCUIT_OPEN_ERROR,
                                        format!("Backend circuit open: {backend_name}"),
                                    );
                                    send_response(&tx, &resp).await;

                                    if let Some(ref atx) = audit_tx {
                                        let entry = AuditEntry {
                                            request_id,
                                            timestamp: chrono::Utc::now(),
                                            client_subject: caller.subject.clone(),
                                            client_role: caller.role.clone(),
                                            tool_name: name.to_string(),
                                            backend_name: backend_name.to_string(),
                                            request_args: request.params.clone(),
                                            response_status: "circuit_open".to_string(),
                                            error_message: Some(format!("Backend circuit open: {backend_name}")),
                                            latency_ms: 0,
                                        };
                                        if let Err(e) = atx.try_send(entry) {
                                            tracing::warn!(error = %e, "Audit channel full, dropping entry");
                                        }
                                    }

                                    continue;
                                }
                            }
                        }
                    }

                    // 6. Backend dispatch
                    let resp =
                        handle_tools_call(id, request.params.clone(), catalog, backends, id_remapper, circuit_breakers)
                            .await;
                    let latency_ms = start.elapsed().as_millis() as i64;
                    let latency_secs = start.elapsed().as_secs_f64();

                    // Record metrics for backend response
                    if let Some(ref m) = metrics {
                        let tool = tool_name.unwrap_or("unknown");
                        let status_str = if resp.error.is_some() { "error" } else { "success" };
                        m.record_request(tool, status_str, latency_secs);
                    }

                    if let Some(ref atx) = audit_tx {
                        let tool = tool_name.unwrap_or("unknown").to_string();
                        let backend = catalog.route(&tool).unwrap_or("unknown").to_string();
                        let (status, error_msg) = if resp.error.is_some() {
                            let msg = resp.error.as_ref().map(|e| e.message.clone());
                            ("error".to_string(), msg)
                        } else {
                            ("success".to_string(), None)
                        };
                        let entry = AuditEntry {
                            request_id,
                            timestamp: chrono::Utc::now(),
                            client_subject: caller.subject.clone(),
                            client_role: caller.role.clone(),
                            tool_name: tool,
                            backend_name: backend,
                            request_args: request.params,
                            response_status: status,
                            error_message: error_msg,
                            latency_ms,
                        };
                        if let Err(e) = atx.try_send(entry) {
                            tracing::warn!(error = %e, "Audit channel full, dropping entry");
                        }
                    }

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
    backends: &HashMap<String, Backend>,
    id_remapper: &IdRemapper,
    circuit_breakers: &HashMap<String, CircuitBreaker>,
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
            // Record success with circuit breaker
            if let Some(cb) = circuit_breakers.get(&backend_name) {
                cb.record_success();
            }
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
            // Record failure with circuit breaker
            if let Some(cb) = circuit_breakers.get(&backend_name) {
                cb.record_failure();
            }
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
