use sentinel_gateway::auth::jwt::{create_token, now_secs, AuthError, Claims, JwtValidator};
use sentinel_gateway::auth::rbac::{is_tool_allowed, Permission};
use sentinel_gateway::config::types::{RbacConfig, RoleConfig};
use std::collections::HashMap;

const TEST_SECRET: &[u8] = b"test-secret-key-for-unit-tests-only";
const ISSUER: &str = "sentinel-gateway";
const AUDIENCE: &str = "sentinel-api";

fn make_validator() -> JwtValidator {
    JwtValidator::new(TEST_SECRET, ISSUER, AUDIENCE)
}

fn make_claims(sub: &str, role: &str, exp_offset_secs: i64) -> Claims {
    let now = now_secs();
    Claims {
        sub: sub.to_string(),
        role: role.to_string(),
        iss: ISSUER.to_string(),
        aud: AUDIENCE.to_string(),
        exp: (now as i64 + exp_offset_secs) as usize,
        iat: Some(now),
        jti: Some("test-jti-001".to_string()),
    }
}

fn make_rbac(roles: Vec<(&str, Vec<&str>, Vec<&str>)>) -> RbacConfig {
    let mut map = HashMap::new();
    for (name, perms, denied) in roles {
        map.insert(
            name.to_string(),
            RoleConfig {
                permissions: perms.into_iter().map(String::from).collect(),
                denied_tools: denied.into_iter().map(String::from).collect(),
            },
        );
    }
    RbacConfig { roles: map }
}

// --- JWT Tests ---

#[test]
fn test_valid_token_accepted() {
    let validator = make_validator();
    let claims = make_claims("user-42", "admin", 3600);
    let token = create_token(&claims, TEST_SECRET).unwrap();

    let identity = validator.validate(&token).unwrap();
    assert_eq!(identity.subject, "user-42");
    assert_eq!(identity.role, "admin");
    assert_eq!(identity.token_id, Some("test-jti-001".to_string()));
}

#[test]
fn test_expired_token_rejected() {
    let validator = make_validator();
    // 120s in the past exceeds the default 60s leeway
    let claims = make_claims("user-42", "admin", -120);
    let token = create_token(&claims, TEST_SECRET).unwrap();

    let err = validator.validate(&token).unwrap_err();
    assert!(matches!(err, AuthError::ExpiredToken));
}

#[test]
fn test_wrong_signature_rejected() {
    let claims = make_claims("user-42", "admin", 3600);
    let token = create_token(&claims, b"wrong-secret").unwrap();

    let validator = make_validator();
    let err = validator.validate(&token).unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
}

#[test]
fn test_wrong_issuer_rejected() {
    let validator = make_validator();
    let mut claims = make_claims("user-42", "admin", 3600);
    claims.iss = "wrong-issuer".to_string();
    let token = create_token(&claims, TEST_SECRET).unwrap();

    let err = validator.validate(&token).unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
}

#[test]
fn test_wrong_audience_rejected() {
    let validator = make_validator();
    let mut claims = make_claims("user-42", "admin", 3600);
    claims.aud = "wrong-audience".to_string();
    let token = create_token(&claims, TEST_SECRET).unwrap();

    let err = validator.validate(&token).unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
}

#[test]
fn test_missing_role_rejected() {
    let validator = make_validator();
    let mut claims = make_claims("user-42", "", 3600);
    claims.role = String::new();
    let token = create_token(&claims, TEST_SECRET).unwrap();

    let err = validator.validate(&token).unwrap_err();
    assert!(matches!(err, AuthError::InvalidClaims(_)));
    assert!(err.to_string().contains("role"));
}

#[test]
fn test_malformed_token_rejected() {
    let validator = make_validator();
    let err = validator.validate("not.a.jwt").unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
}

#[test]
fn test_create_and_validate_roundtrip() {
    let validator = make_validator();
    let claims = Claims {
        sub: "roundtrip-user".to_string(),
        role: "operator".to_string(),
        iss: ISSUER.to_string(),
        aud: AUDIENCE.to_string(),
        exp: now_secs() + 3600,
        iat: Some(now_secs()),
        jti: Some("jti-roundtrip".to_string()),
    };
    let token = create_token(&claims, TEST_SECRET).unwrap();
    let identity = validator.validate(&token).unwrap();

    assert_eq!(identity.subject, "roundtrip-user");
    assert_eq!(identity.role, "operator");
    assert_eq!(identity.token_id, Some("jti-roundtrip".to_string()));
}

#[test]
fn test_json_rpc_code() {
    let err = AuthError::MissingToken;
    assert_eq!(err.json_rpc_code(), -32001);

    let err = AuthError::ExpiredToken;
    assert_eq!(err.json_rpc_code(), -32001);
}

// --- RBAC Tests ---

#[test]
fn test_admin_wildcard_allows_all() {
    let rbac = make_rbac(vec![("admin", vec!["*"], vec![])]);
    assert!(is_tool_allowed("admin", "anything", Permission::Read, &rbac));
    assert!(is_tool_allowed(
        "admin",
        "anything",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_unknown_role_denied() {
    let rbac = make_rbac(vec![("admin", vec!["*"], vec![])]);
    assert!(!is_tool_allowed(
        "ghost",
        "anything",
        Permission::Read,
        &rbac
    ));
    assert!(!is_tool_allowed(
        "ghost",
        "anything",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_denied_tools_override_wildcard() {
    let rbac = make_rbac(vec![("admin", vec!["*"], vec!["secret_tool"])]);
    assert!(!is_tool_allowed(
        "admin",
        "secret_tool",
        Permission::Execute,
        &rbac
    ));
    assert!(!is_tool_allowed(
        "admin",
        "secret_tool",
        Permission::Read,
        &rbac
    ));
    // Other tools still allowed
    assert!(is_tool_allowed(
        "admin",
        "normal_tool",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_read_permission_allows_list() {
    let rbac = make_rbac(vec![("viewer", vec!["tools.read"], vec![])]);
    assert!(is_tool_allowed(
        "viewer",
        "some_tool",
        Permission::Read,
        &rbac
    ));
    assert!(!is_tool_allowed(
        "viewer",
        "some_tool",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_execute_permission_implies_read() {
    let rbac = make_rbac(vec![("operator", vec!["tools.execute"], vec![])]);
    assert!(is_tool_allowed(
        "operator",
        "some_tool",
        Permission::Read,
        &rbac
    ));
    assert!(is_tool_allowed(
        "operator",
        "some_tool",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_empty_permissions_denied() {
    let rbac = make_rbac(vec![("empty", vec![], vec![])]);
    assert!(!is_tool_allowed(
        "empty",
        "some_tool",
        Permission::Read,
        &rbac
    ));
    assert!(!is_tool_allowed(
        "empty",
        "some_tool",
        Permission::Execute,
        &rbac
    ));
}

#[test]
fn test_denied_tools_with_execute_permission() {
    let rbac = make_rbac(vec![(
        "operator",
        vec!["tools.execute"],
        vec!["forbidden_tool"],
    )]);
    assert!(!is_tool_allowed(
        "operator",
        "forbidden_tool",
        Permission::Execute,
        &rbac
    ));
    assert!(!is_tool_allowed(
        "operator",
        "forbidden_tool",
        Permission::Read,
        &rbac
    ));
    // Other tools still allowed
    assert!(is_tool_allowed(
        "operator",
        "allowed_tool",
        Permission::Execute,
        &rbac
    ));
}
