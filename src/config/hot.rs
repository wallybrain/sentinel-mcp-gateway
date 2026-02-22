use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::load_config_lenient;
use crate::config::types::KillSwitchConfig;
use crate::ratelimit::RateLimiter;

pub struct HotConfig {
    pub kill_switch: KillSwitchConfig,
    pub rate_limiter: RateLimiter,
}

pub type SharedHotConfig = Arc<RwLock<HotConfig>>;

impl HotConfig {
    pub fn new(kill_switch: KillSwitchConfig, rate_limiter: RateLimiter) -> Self {
        Self {
            kill_switch,
            rate_limiter,
        }
    }

    pub fn shared(self) -> SharedHotConfig {
        Arc::new(RwLock::new(self))
    }
}

pub fn reload_hot_config(config_path: &str) -> Result<HotConfig, anyhow::Error> {
    let config = load_config_lenient(config_path)?;
    let rate_limiter = RateLimiter::new(&config.rate_limits);
    Ok(HotConfig::new(config.kill_switch, rate_limiter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::RateLimitConfig;
    use std::io::Write;

    #[test]
    fn test_hot_config_new() {
        let ks = KillSwitchConfig {
            disabled_tools: vec!["dangerous_tool".to_string()],
            disabled_backends: vec![],
        };
        let rl = RateLimiter::new(&RateLimitConfig::default());
        let hot = HotConfig::new(ks, rl);

        assert_eq!(hot.kill_switch.disabled_tools, vec!["dangerous_tool"]);
    }

    #[test]
    fn test_reload_hot_config_valid() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            r#"
[gateway]
listen = "127.0.0.1:9200"
log_level = "info"

[auth]
jwt_secret_env = "JWT_SECRET"

[postgres]
url_env = "DATABASE_URL"

[kill_switch]
disabled_tools = ["blocked_tool"]
disabled_backends = []

[rate_limits]
default_rpm = 500
"#
        )
        .unwrap();

        let hot = reload_hot_config(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(hot.kill_switch.disabled_tools, vec!["blocked_tool"]);
        // Rate limiter is created from config -- verify it works
        assert!(hot.rate_limiter.check("client", "any_tool").is_ok());
    }

    #[test]
    fn test_reload_hot_config_invalid_file() {
        let result = reload_hot_config("/nonexistent/sentinel.toml");
        assert!(result.is_err());
    }
}
