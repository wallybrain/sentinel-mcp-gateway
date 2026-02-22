use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct SentinelConfig {
    pub gateway: GatewayConfig,
    pub auth: AuthConfig,
    pub postgres: PostgresConfig,
    #[serde(default)]
    pub backends: Vec<BackendConfig>,
    #[serde(default)]
    pub rbac: RbacConfig,
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
    #[serde(default)]
    pub kill_switch: KillSwitchConfig,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub audit_enabled: bool,
    #[serde(default = "default_health_listen")]
    pub health_listen: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret_env: String,
    #[serde(default = "default_issuer")]
    pub jwt_issuer: String,
    #[serde(default = "default_audience")]
    pub jwt_audience: String,
}

#[derive(Debug, Deserialize)]
pub struct PostgresConfig {
    pub url_env: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub backend_type: BackendType,
    pub url: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default)]
    pub restart_on_exit: bool,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    #[serde(default = "default_health_interval")]
    pub health_interval_secs: u64,
    #[serde(default = "default_cb_threshold")]
    pub circuit_breaker_threshold: u32,
    #[serde(default = "default_cb_recovery")]
    pub circuit_breaker_recovery_secs: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    Http,
    Stdio,
}

#[derive(Debug, Default, Deserialize)]
pub struct RbacConfig {
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RoleConfig {
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rpm")]
    pub default_rpm: u32,
    #[serde(default)]
    pub per_tool: HashMap<String, u32>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            default_rpm: default_rpm(),
            per_tool: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KillSwitchConfig {
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_backends: Vec<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            log_level: default_log_level(),
            audit_enabled: default_true(),
            health_listen: default_health_listen(),
        }
    }
}

fn default_listen() -> String {
    "127.0.0.1:9200".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_true() -> bool {
    true
}
fn default_issuer() -> String {
    "sentinel-gateway".to_string()
}
fn default_audience() -> String {
    "sentinel-api".to_string()
}
fn default_max_connections() -> u32 {
    10
}
fn default_timeout() -> u64 {
    60
}
fn default_retries() -> u32 {
    3
}
fn default_max_restarts() -> u32 {
    5
}
fn default_health_interval() -> u64 {
    300
}
fn default_rpm() -> u32 {
    1000
}
fn default_health_listen() -> String {
    "127.0.0.1:9201".to_string()
}
fn default_cb_threshold() -> u32 {
    5
}
fn default_cb_recovery() -> u64 {
    30
}
