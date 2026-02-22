use rmcp::model::{
    Implementation, InitializeRequestParams, InitializeResult, ProtocolVersion,
    ServerCapabilities,
};
use serde_json::Value;

use super::jsonrpc::INVALID_PARAMS;

/// MCP lifecycle states per the MCP spec 2025-03-26.
///
/// The gateway progresses through these states:
/// Created -> Initializing -> Operational -> Closed
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpState {
    Created,
    Initializing,
    Operational,
    Closed,
}

impl McpState {
    /// Returns whether the given method is allowed in the current state.
    ///
    /// - `Created`: only "initialize" and "ping"
    /// - `Initializing`: only "notifications/initialized" and "ping"
    /// - `Operational`: all methods accepted
    /// - `Closed`: nothing accepted
    pub fn can_accept_method(&self, method: &str) -> bool {
        match self {
            McpState::Created => method == "initialize" || method == "ping",
            McpState::Initializing => {
                method == "notifications/initialized" || method == "ping"
            }
            McpState::Operational => true,
            McpState::Closed => false,
        }
    }
}

/// Handles an MCP `initialize` request.
///
/// Deserializes the params into `InitializeRequestParams`, logs client info,
/// and returns a spec-compliant `InitializeResult` with protocol version
/// 2025-03-26 and tools capability enabled.
pub fn handle_initialize(params: Value) -> Result<Value, (i32, String)> {
    let init_params: InitializeRequestParams = serde_json::from_value(params).map_err(|e| {
        (
            INVALID_PARAMS,
            format!("Invalid initialize params: {e}"),
        )
    })?;

    tracing::info!(
        client_name = %init_params.client_info.name,
        client_version = %init_params.client_info.version,
        "MCP initialize from client"
    );

    let capabilities = ServerCapabilities::builder().enable_tools().build();

    let result = InitializeResult {
        protocol_version: ProtocolVersion::V_2025_03_26,
        capabilities,
        server_info: Implementation {
            name: "sentinel-gateway".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: None,
            description: None,
            icons: None,
            website_url: None,
        },
        instructions: Some("Sentinel Gateway - governed MCP tool access".into()),
    };

    serde_json::to_value(&result).map_err(|e| {
        (
            super::jsonrpc::INTERNAL_ERROR,
            format!("Failed to serialize initialize result: {e}"),
        )
    })
}
