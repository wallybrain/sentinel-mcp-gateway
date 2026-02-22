use sentinel_gateway::catalog::{create_stub_catalog, ToolCatalog};
use std::sync::Arc;

fn make_tool(name: &str, description: &str) -> rmcp::model::Tool {
    let schema: serde_json::Map<String, serde_json::Value> = serde_json::from_value(
        serde_json::json!({"type": "object", "properties": {}}),
    )
    .unwrap();
    rmcp::model::Tool::new(
        name.to_string(),
        description.to_string(),
        Arc::new(schema),
    )
}

#[test]
fn test_register_and_list_tools() {
    let mut catalog = ToolCatalog::new();
    catalog.register_backend(
        "backend-a",
        vec![make_tool("tool1", "desc1"), make_tool("tool2", "desc2")],
    );
    catalog.register_backend(
        "backend-b",
        vec![make_tool("tool3", "desc3"), make_tool("tool4", "desc4")],
    );
    assert_eq!(catalog.tool_count(), 4);
    assert_eq!(catalog.all_tools().len(), 4);
}

#[test]
fn test_route_returns_correct_backend() {
    let mut catalog = ToolCatalog::new();
    catalog.register_backend(
        "stub-n8n",
        vec![make_tool("list_workflows", "List workflows")],
    );
    catalog.register_backend(
        "stub-sqlite",
        vec![make_tool("read_query", "Read query")],
    );
    assert_eq!(catalog.route("list_workflows"), Some("stub-n8n"));
    assert_eq!(catalog.route("read_query"), Some("stub-sqlite"));
}

#[test]
fn test_route_unknown_tool_returns_none() {
    let catalog = ToolCatalog::new();
    assert_eq!(catalog.route("nonexistent"), None);
}

#[test]
fn test_name_collision_prefixes() {
    let mut catalog = ToolCatalog::new();
    catalog.register_backend("alpha", vec![make_tool("query", "Alpha query")]);
    catalog.register_backend("beta", vec![make_tool("query", "Beta query")]);
    assert_eq!(catalog.tool_count(), 2);
    // First registration gets the bare name
    assert_eq!(catalog.route("query"), Some("alpha"));
    // Second registration gets prefixed
    assert_eq!(catalog.route("beta__query"), Some("beta"));
}

#[test]
fn test_stub_catalog_has_expected_tools() {
    let catalog = create_stub_catalog();
    assert_eq!(catalog.tool_count(), 4);
    assert_eq!(catalog.route("list_workflows"), Some("stub-n8n"));
    assert_eq!(catalog.route("execute_workflow"), Some("stub-n8n"));
    assert_eq!(catalog.route("read_query"), Some("stub-sqlite"));
    assert_eq!(catalog.route("write_query"), Some("stub-sqlite"));
}
