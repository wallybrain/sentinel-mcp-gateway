pub mod secrets;
pub mod types;

pub use types::*;

use anyhow::Context;
use std::collections::HashSet;

pub fn load_config(path: &str) -> Result<SentinelConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;

    let config: SentinelConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {path}"))?;

    config.validate()?;
    Ok(config)
}

/// Loads config without requiring auth/postgres secrets to be present.
///
/// Validates backend definitions but skips JWT and database URL resolution.
/// Used during early phases when auth and persistence aren't wired yet.
pub fn load_config_lenient(path: &str) -> Result<SentinelConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {path}"))?;

    let config: SentinelConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {path}"))?;

    config.validate_backends()?;
    Ok(config)
}

impl SentinelConfig {
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        self.auth
            .resolve_jwt_secret()
            .with_context(|| "Config validation failed")?;

        self.postgres
            .resolve_url()
            .with_context(|| "Config validation failed")?;

        self.validate_backends()?;
        Ok(())
    }

    /// Validates only backend definitions (no auth/postgres secrets needed).
    pub fn validate_backends(&self) -> Result<(), anyhow::Error> {
        let mut names = HashSet::new();
        for backend in &self.backends {
            if !names.insert(&backend.name) {
                anyhow::bail!("Duplicate backend name: {}", backend.name);
            }

            match backend.backend_type {
                BackendType::Http => {
                    if backend.url.is_none() {
                        anyhow::bail!(
                            "HTTP backend '{}' must have a 'url' field",
                            backend.name
                        );
                    }
                }
                BackendType::Stdio => {
                    if backend.command.is_none() {
                        anyhow::bail!(
                            "stdio backend '{}' must have a 'command' field",
                            backend.name
                        );
                    }
                }
            }
        }

        Ok(())
    }
}
