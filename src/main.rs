use clap::Parser;
use tokio::sync::mpsc;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = sentinel_gateway::cli::Cli::parse();

    let config = sentinel_gateway::config::load_config_lenient(&cli.config)?;

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

    let catalog = sentinel_gateway::catalog::create_stub_catalog();
    tracing::info!(tools = catalog.tool_count(), "Tool catalog loaded");

    let (inbound_tx, inbound_rx) = mpsc::channel::<String>(64);
    let (outbound_tx, outbound_rx) = mpsc::channel::<String>(64);

    tokio::spawn(sentinel_gateway::transport::stdio::stdio_reader(inbound_tx));
    tokio::spawn(sentinel_gateway::transport::stdio::stdio_writer(outbound_rx));

    sentinel_gateway::gateway::run_dispatch(inbound_rx, outbound_tx, &catalog).await?;

    tracing::info!("Dispatch loop ended (stdin closed)");
    Ok(())
}
