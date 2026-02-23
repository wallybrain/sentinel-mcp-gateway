use sentinel_gateway::backend::{parse_sse_data, BackendError, HttpBackend, build_http_client};
use sentinel_gateway::config::types::{BackendConfig, BackendType};
use std::collections::HashMap;

// --- SSE parsing tests ---

#[test]
fn test_parse_sse_basic() {
    let input = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1}\n\n";
    let result = parse_sse_data(input);
    assert_eq!(result, Some("{\"jsonrpc\":\"2.0\",\"id\":1}".to_string()));
}

#[test]
fn test_parse_sse_no_data_line() {
    let input = "event: message\n\n";
    let result = parse_sse_data(input);
    assert_eq!(result, None);
}

#[test]
fn test_parse_sse_empty_data() {
    let input = "data: \n\n";
    let result = parse_sse_data(input);
    assert_eq!(result, None);
}

#[test]
fn test_parse_sse_multiple_data_lines() {
    let input = "data: \ndata: {\"first\":true}\ndata: {\"second\":true}\n\n";
    let result = parse_sse_data(input);
    assert_eq!(result, Some("{\"first\":true}".to_string()));
}

#[test]
fn test_parse_sse_with_whitespace() {
    let input = "data:  {\"result\":{}}\n";
    let result = parse_sse_data(input);
    assert_eq!(result, Some("{\"result\":{}}".to_string()));
}

// --- Error classification tests ---

#[test]
fn test_backend_error_retryable_http_500() {
    let err = BackendError::HttpStatus(500, "Internal Server Error".to_string());
    assert!(err.is_retryable());
}

#[test]
fn test_backend_error_not_retryable_http_400() {
    let err = BackendError::HttpStatus(400, "Bad Request".to_string());
    assert!(!err.is_retryable());
}

#[test]
fn test_backend_error_not_retryable_no_data() {
    let err = BackendError::NoDataInSse;
    assert!(!err.is_retryable());
}

#[test]
fn test_backend_error_not_retryable_invalid_response() {
    let err = BackendError::InvalidResponse("bad json".to_string());
    assert!(!err.is_retryable());
}

// --- HttpBackend construction tests ---

fn make_config(url: &str) -> BackendConfig {
    BackendConfig {
        name: "test".to_string(),
        backend_type: BackendType::Http,
        url: Some(url.to_string()),
        command: None,
        args: vec![],
        env: HashMap::new(),
        timeout_secs: 60,
        retries: 3,
        restart_on_exit: false,
        max_restarts: 5,
        health_interval_secs: 300,
        circuit_breaker_threshold: 5,
        circuit_breaker_recovery_secs: 30,
    }
}

#[test]
fn test_http_backend_appends_mcp_path() {
    let client = build_http_client().unwrap();
    let config = make_config("http://localhost:3000");
    let backend = HttpBackend::new(client, &config, None);
    assert_eq!(backend.url(), "http://localhost:3000/mcp");
}

#[test]
fn test_http_backend_preserves_mcp_path() {
    let client = build_http_client().unwrap();
    let config = make_config("http://localhost:3000/mcp");
    let backend = HttpBackend::new(client, &config, None);
    assert_eq!(backend.url(), "http://localhost:3000/mcp");
}

#[test]
fn test_http_backend_strips_trailing_slash() {
    let client = build_http_client().unwrap();
    let config = make_config("http://localhost:3000/");
    let backend = HttpBackend::new(client, &config, None);
    assert_eq!(backend.url(), "http://localhost:3000/mcp");
}

// --- Build client test ---

#[test]
fn test_build_http_client() {
    let result = build_http_client();
    assert!(result.is_ok());
}
