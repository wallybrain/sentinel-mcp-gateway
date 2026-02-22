use std::io::Write;

fn temp_config_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("sentinel_test_{name}_{}.toml", std::process::id()));
    path
}

fn minimal_valid_config(jwt_env: &str, db_env: &str) -> String {
    format!(
        r#"
[gateway]
listen = "127.0.0.1:9200"

[auth]
jwt_secret_env = "{jwt_env}"
jwt_issuer = "test"
jwt_audience = "test"

[postgres]
url_env = "{db_env}"

[[backends]]
name = "test-http"
type = "http"
url = "http://localhost:3000"

[[backends]]
name = "test-stdio"
type = "stdio"
command = "echo"
"#
    )
}

fn write_temp_config(name: &str, content: &str) -> std::path::PathBuf {
    let path = temp_config_path(name);
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    path
}

#[test]
fn valid_config_loads() {
    let jwt_env = "TEST_JWT_VALID_CONFIG";
    let db_env = "TEST_DB_VALID_CONFIG";
    unsafe {
        std::env::set_var(jwt_env, "super-secret");
        std::env::set_var(db_env, "postgres://test:test@localhost/test");
    }

    let path = write_temp_config("valid", &minimal_valid_config(jwt_env, db_env));
    let config = sentinel_gateway::config::load_config(path.to_str().unwrap()).unwrap();

    assert_eq!(config.gateway.listen, "127.0.0.1:9200");
    assert_eq!(config.auth.jwt_secret_env, jwt_env);
    assert_eq!(config.postgres.url_env, db_env);
    assert_eq!(config.backends.len(), 2);
    assert_eq!(config.backends[0].name, "test-http");
    assert_eq!(config.backends[1].name, "test-stdio");

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::remove_var(db_env);
    }
}

#[test]
fn missing_config_file_errors() {
    let result = sentinel_gateway::config::load_config("nonexistent_test_file.toml");
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("nonexistent_test_file.toml"),
        "Error should mention the file path: {err}"
    );
}

#[test]
fn malformed_toml_errors() {
    let path = write_temp_config("malformed", "this is [[[not valid toml");
    let result = sentinel_gateway::config::load_config(path.to_str().unwrap());
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("parse") || err.contains("expected"),
        "Error should mention parse failure: {err}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn missing_jwt_secret_env_errors() {
    let jwt_env = "TEST_JWT_MISSING_SECRET";
    let db_env = "TEST_DB_MISSING_SECRET";
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::set_var(db_env, "postgres://test");
    }

    let path = write_temp_config("missing_jwt", &minimal_valid_config(jwt_env, db_env));
    let result = sentinel_gateway::config::load_config(path.to_str().unwrap());
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains(jwt_env),
        "Error should mention the env var name: {err}"
    );

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(db_env);
    }
}

#[test]
fn duplicate_backend_names_errors() {
    let jwt_env = "TEST_JWT_DUP_BACKEND";
    let db_env = "TEST_DB_DUP_BACKEND";
    unsafe {
        std::env::set_var(jwt_env, "secret");
        std::env::set_var(db_env, "postgres://test");
    }

    let config_str = format!(
        r#"
[gateway]
[auth]
jwt_secret_env = "{jwt_env}"
[postgres]
url_env = "{db_env}"

[[backends]]
name = "n8n"
type = "http"
url = "http://localhost:3000"

[[backends]]
name = "n8n"
type = "http"
url = "http://localhost:3001"
"#
    );

    let path = write_temp_config("dup_backends", &config_str);
    let result = sentinel_gateway::config::load_config(path.to_str().unwrap());
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("Duplicate backend name"),
        "Error should mention duplicate: {err}"
    );

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::remove_var(db_env);
    }
}

#[test]
fn http_backend_without_url_errors() {
    let jwt_env = "TEST_JWT_HTTP_NO_URL";
    let db_env = "TEST_DB_HTTP_NO_URL";
    unsafe {
        std::env::set_var(jwt_env, "secret");
        std::env::set_var(db_env, "postgres://test");
    }

    let config_str = format!(
        r#"
[gateway]
[auth]
jwt_secret_env = "{jwt_env}"
[postgres]
url_env = "{db_env}"

[[backends]]
name = "broken-http"
type = "http"
"#
    );

    let path = write_temp_config("http_no_url", &config_str);
    let result = sentinel_gateway::config::load_config(path.to_str().unwrap());
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("url"),
        "Error should mention missing url: {err}"
    );

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::remove_var(db_env);
    }
}

#[test]
fn stdio_backend_without_command_errors() {
    let jwt_env = "TEST_JWT_STDIO_NO_CMD";
    let db_env = "TEST_DB_STDIO_NO_CMD";
    unsafe {
        std::env::set_var(jwt_env, "secret");
        std::env::set_var(db_env, "postgres://test");
    }

    let config_str = format!(
        r#"
[gateway]
[auth]
jwt_secret_env = "{jwt_env}"
[postgres]
url_env = "{db_env}"

[[backends]]
name = "broken-stdio"
type = "stdio"
"#
    );

    let path = write_temp_config("stdio_no_cmd", &config_str);
    let result = sentinel_gateway::config::load_config(path.to_str().unwrap());
    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("command"),
        "Error should mention missing command: {err}"
    );

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::remove_var(db_env);
    }
}

#[test]
fn defaults_applied() {
    let jwt_env = "TEST_JWT_DEFAULTS";
    let db_env = "TEST_DB_DEFAULTS";
    unsafe {
        std::env::set_var(jwt_env, "secret");
        std::env::set_var(db_env, "postgres://test");
    }

    let config_str = format!(
        r#"
[gateway]
[auth]
jwt_secret_env = "{jwt_env}"
[postgres]
url_env = "{db_env}"
"#
    );

    let path = write_temp_config("defaults", &config_str);
    let config = sentinel_gateway::config::load_config(path.to_str().unwrap()).unwrap();

    assert_eq!(config.gateway.listen, "127.0.0.1:9200");
    assert_eq!(config.gateway.log_level, "info");
    assert!(config.gateway.audit_enabled);
    assert_eq!(config.rate_limits.default_rpm, 1000);
    assert_eq!(config.postgres.max_connections, 10);
    assert!(config.backends.is_empty());
    assert!(config.rbac.roles.is_empty());
    assert!(config.kill_switch.disabled_tools.is_empty());

    std::fs::remove_file(&path).ok();
    unsafe {
        std::env::remove_var(jwt_env);
        std::env::remove_var(db_env);
    }
}
