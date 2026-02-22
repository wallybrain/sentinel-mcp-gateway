use crate::config::types::{AuthConfig, PostgresConfig};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing secret: environment variable '{env_var}' not set ({context})")]
    MissingSecret { env_var: String, context: String },

    #[error("Missing config file: {path}")]
    MissingConfig { path: String },
}

impl AuthConfig {
    pub fn resolve_jwt_secret(&self) -> Result<String, ConfigError> {
        std::env::var(&self.jwt_secret_env).map_err(|_| ConfigError::MissingSecret {
            env_var: self.jwt_secret_env.clone(),
            context: "JWT secret key".to_string(),
        })
    }
}

impl PostgresConfig {
    pub fn resolve_url(&self) -> Result<String, ConfigError> {
        std::env::var(&self.url_env).map_err(|_| ConfigError::MissingSecret {
            env_var: self.url_env.clone(),
            context: "PostgreSQL connection URL".to_string(),
        })
    }
}
