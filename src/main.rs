use anyhow::Context;
use clap::Parser;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = sentinel_gateway::cli::Cli::parse();

    let config = sentinel_gateway::config::load_config(&cli.config)
        .context("Failed to load configuration")?;

    let log_level = cli
        .log_level
        .as_deref()
        .unwrap_or(&config.gateway.log_level);
    sentinel_gateway::logging::init(log_level);

    tracing::info!(
        listen = %config.gateway.listen,
        backends = config.backends.len(),
        "Sentinel Gateway starting"
    );
    tracing::info!("Configuration loaded and validated");
    tracing::info!("Foundation phase complete. No transport configured yet.");

    Ok(())
}
