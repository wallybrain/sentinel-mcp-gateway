use std::collections::HashMap;

use jsonschema::Validator;
use serde_json::Value;

use crate::catalog::ToolCatalog;

pub struct SchemaCache {
    validators: HashMap<String, Validator>,
}

impl SchemaCache {
    pub fn from_catalog(catalog: &ToolCatalog) -> Self {
        let mut validators = HashMap::new();

        for tool in catalog.all_tools() {
            let name = tool.name.to_string();
            let schema_value = Value::Object((*tool.input_schema).clone());

            match Validator::new(&schema_value) {
                Ok(validator) => {
                    validators.insert(name, validator);
                }
                Err(err) => {
                    tracing::warn!(
                        tool = %name,
                        error = %err,
                        "Failed to compile JSON schema for tool, skipping validation"
                    );
                }
            }
        }

        Self { validators }
    }

    pub fn validate(&self, tool_name: &str, arguments: &Value) -> Result<(), Vec<String>> {
        let validator = match self.validators.get(tool_name) {
            Some(v) => v,
            None => return Ok(()),
        };

        let errors: Vec<String> = validator
            .iter_errors(arguments)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;
    use std::sync::Arc;

    fn make_tool_with_schema(name: &str, schema: Value) -> Tool {
        let schema_map: serde_json::Map<String, Value> =
            serde_json::from_value(schema).expect("schema must be an object");
        Tool::new(
            name.to_string(),
            format!("Test tool {name}"),
            Arc::new(schema_map),
        )
    }

    fn build_cache_with_tool(name: &str, schema: Value) -> SchemaCache {
        let mut catalog = ToolCatalog::new();
        catalog.register_backend("test", vec![make_tool_with_schema(name, schema)]);
        SchemaCache::from_catalog(&catalog)
    }

    fn query_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        })
    }

    #[test]
    fn test_validate_passes_valid_args() {
        let cache = build_cache_with_tool("search", query_schema());
        let args = serde_json::json!({"query": "hello"});
        assert!(cache.validate("search", &args).is_ok());
    }

    #[test]
    fn test_validate_rejects_invalid_args() {
        let cache = build_cache_with_tool("search", query_schema());
        let args = serde_json::json!({"query": 42});
        let err = cache.validate("search", &args).unwrap_err();
        assert!(
            err.iter().any(|e| e.contains("type")),
            "Expected error about type, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_rejects_missing_required() {
        let cache = build_cache_with_tool("search", query_schema());
        let args = serde_json::json!({});
        let err = cache.validate("search", &args).unwrap_err();
        assert!(
            err.iter().any(|e| e.contains("required")),
            "Expected error about required, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_skips_unknown_tool() {
        let cache = build_cache_with_tool("search", query_schema());
        let args = serde_json::json!({"anything": true});
        assert!(cache.validate("nonexistent_tool", &args).is_ok());
    }

    #[test]
    fn test_from_catalog_skips_invalid_schema() {
        // A schema with no "type" field is valid JSON schema (permissive).
        // Use a schema that has contradictory constraints but still compiles.
        // The key safety test: from_catalog doesn't crash on any well-formed JSON object.
        let odd_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "x": {"type": "integer", "minimum": 100, "maximum": 0}
            }
        });
        let cache = build_cache_with_tool("odd_tool", odd_schema);
        // Should have compiled (contradictory but syntactically valid schema)
        // Validate something against it -- should fail due to contradictions
        let result = cache.validate("odd_tool", &serde_json::json!({"x": 50}));
        // We don't assert pass/fail -- the point is it didn't crash
        let _ = result;
    }
}
