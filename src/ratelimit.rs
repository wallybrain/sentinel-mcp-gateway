use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::types::RateLimitConfig;

struct TokenBucket {
    tokens: u32,
    max_tokens: u32,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u32) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> Result<(), f64> {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        if elapsed >= 60.0 {
            self.tokens = self.max_tokens;
            self.last_refill = Instant::now();
        }

        if self.tokens > 0 {
            self.tokens -= 1;
            Ok(())
        } else {
            let retry = (60.0 - elapsed).max(1.0);
            Err(retry)
        }
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<(String, String), TokenBucket>>,
    default_rpm: u32,
    per_tool: HashMap<String, u32>,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            default_rpm: config.default_rpm,
            per_tool: config.per_tool.clone(),
        }
    }

    pub fn check(&self, client: &str, tool: &str) -> Result<(), f64> {
        let rpm = self.per_tool.get(tool).copied().unwrap_or(self.default_rpm);
        let key = (client.to_string(), tool.to_string());
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets.entry(key).or_insert_with(|| TokenBucket::new(rpm));
        bucket.try_consume()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(default_rpm: u32, per_tool: Vec<(&str, u32)>) -> RateLimitConfig {
        RateLimitConfig {
            default_rpm,
            per_tool: per_tool.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    #[test]
    fn rate_limiter_allows_within_limit() {
        let config = make_config(5, vec![]);
        let limiter = RateLimiter::new(&config);

        for _ in 0..5 {
            assert!(limiter.check("client1", "tool_a").is_ok());
        }
        assert!(limiter.check("client1", "tool_a").is_err());

        // Different client is unaffected
        assert!(limiter.check("client2", "tool_a").is_ok());
    }

    #[test]
    fn rate_limiter_per_tool_override() {
        let config = make_config(10, vec![("expensive_tool", 2)]);
        let limiter = RateLimiter::new(&config);

        assert!(limiter.check("client1", "expensive_tool").is_ok());
        assert!(limiter.check("client1", "expensive_tool").is_ok());
        assert!(limiter.check("client1", "expensive_tool").is_err());

        // Default-limit tool still works
        assert!(limiter.check("client1", "normal_tool").is_ok());
    }

    #[test]
    fn rate_limiter_returns_positive_retry_after() {
        let config = make_config(1, vec![]);
        let limiter = RateLimiter::new(&config);

        assert!(limiter.check("client1", "tool_a").is_ok());
        let err = limiter.check("client1", "tool_a").unwrap_err();
        assert!(err > 0.0, "retry_after should be positive, got {err}");
        assert!(err <= 60.0, "retry_after should be <= 60.0, got {err}");
    }

    #[test]
    fn rate_limiter_independent_per_client() {
        let config = make_config(1, vec![]);
        let limiter = RateLimiter::new(&config);

        assert!(limiter.check("client1", "tool_a").is_ok());
        assert!(limiter.check("client1", "tool_a").is_err());

        // client2 for same tool is unaffected
        assert!(limiter.check("client2", "tool_a").is_ok());
    }
}
