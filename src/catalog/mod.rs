use std::collections::HashMap;
use std::sync::Arc;

use rmcp::model::Tool;

/// Aggregates tools from multiple MCP backends into a single unified catalog.
///
/// Tracks which backend owns each tool for routing tool/call requests
/// to the correct backend in later phases.
pub struct ToolCatalog {
    /// Maps tool_name -> (tool_definition, backend_name)
    tools: HashMap<String, (Tool, String)>,
}

impl Default for ToolCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolCatalog {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Registers all tools from a backend. On name collision, the new tool
    /// is prefixed with `{backend_name}__{tool_name}`.
    pub fn register_backend(&mut self, backend_name: &str, tools: Vec<Tool>) {
        for tool in tools {
            let name = tool.name.to_string();
            if self.tools.contains_key(&name) {
                let prefixed = format!("{backend_name}__{name}");
                tracing::warn!(
                    original = %name,
                    prefixed = %prefixed,
                    backend = %backend_name,
                    "Tool name collision, prefixing with backend name"
                );
                let mut prefixed_tool = tool;
                prefixed_tool.name = prefixed.clone().into();
                self.tools
                    .insert(prefixed, (prefixed_tool, backend_name.to_string()));
            } else {
                self.tools
                    .insert(name, (tool, backend_name.to_string()));
            }
        }
    }

    /// Returns all tool definitions from all backends.
    pub fn all_tools(&self) -> Vec<Tool> {
        self.tools.values().map(|(tool, _)| tool.clone()).collect()
    }

    /// Returns the backend name that owns the given tool.
    pub fn route(&self, tool_name: &str) -> Option<&str> {
        self.tools.get(tool_name).map(|(_, backend)| backend.as_str())
    }

    /// Returns the total number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

fn make_tool(name: &str, description: &str) -> Tool {
    let schema: serde_json::Map<String, serde_json::Value> = serde_json::from_value(
        serde_json::json!({"type": "object", "properties": {}}),
    )
    .expect("valid schema");
    Tool::new(name.to_string(), description.to_string(), Arc::new(schema))
}

/// Creates a stub catalog with test backends for development and testing.
///
/// - "stub-n8n" backend: list_workflows, execute_workflow
/// - "stub-sqlite" backend: read_query, write_query
pub fn create_stub_catalog() -> ToolCatalog {
    let mut catalog = ToolCatalog::new();

    let n8n_tools = vec![
        make_tool("list_workflows", "List n8n workflows"),
        make_tool("execute_workflow", "Execute an n8n workflow"),
    ];
    catalog.register_backend("stub-n8n", n8n_tools);

    let sqlite_tools = vec![
        make_tool("read_query", "Execute a read-only SQL query"),
        make_tool("write_query", "Execute a write SQL query"),
    ];
    catalog.register_backend("stub-sqlite", sqlite_tools);

    catalog
}
