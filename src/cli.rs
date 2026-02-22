use clap::Parser;

#[derive(Parser)]
#[command(name = "sentinel-gateway", about = "MCP Gateway with auth, routing, and audit")]
pub struct Cli {
    /// Path to sentinel.toml config file
    #[arg(long, default_value = "sentinel.toml", env = "SENTINEL_CONFIG")]
    pub config: String,

    /// Override log level (trace, debug, info, warn, error)
    #[arg(long, env = "LOG_LEVEL")]
    pub log_level: Option<String>,
}
