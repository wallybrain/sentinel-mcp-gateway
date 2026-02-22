use std::collections::HashMap;

use clap::Parser;
use tokio::sync::mpsc;

use sentinel_gateway::auth::jwt::{CallerIdentity, JwtValidator};
use sentinel_gateway::backend::{build_http_client, discover_tools, HttpBackend};
use sentinel_gateway::config::types::BackendType;
use sentinel_gateway::protocol::id_remapper::IdRemapper;

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

    // Build shared HTTP client and discover backends
    let mut catalog = sentinel_gateway::catalog::ToolCatalog::new();
    let mut backends_map: HashMap<String, HttpBackend> = HashMap::new();
    let mut discovery_succeeded = false;

    let http_backends: Vec<_> = config
        .backends
        .iter()
        .filter(|b| b.backend_type == BackendType::Http)
        .collect();

    if !http_backends.is_empty() {
        match build_http_client() {
            Ok(client) => {
                for backend_config in &http_backends {
                    let backend = HttpBackend::new(client.clone(), backend_config);
                    tracing::info!(
                        name = %backend_config.name,
                        url = %backend.url(),
                        "Discovering tools from backend"
                    );

                    match discover_tools(&backend).await {
                        Ok(tools) => {
                            let tool_count = tools.len();
                            catalog.register_backend(&backend_config.name, tools);
                            backends_map.insert(backend_config.name.clone(), backend);
                            discovery_succeeded = true;
                            tracing::info!(
                                name = %backend_config.name,
                                tools = tool_count,
                                "Backend registered"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                name = %backend_config.name,
                                error = %e,
                                "Failed to discover tools from backend, skipping"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to build HTTP client");
            }
        }
    }

    if !discovery_succeeded {
        tracing::warn!("No HTTP backends available, falling back to stub catalog");
        catalog = sentinel_gateway::catalog::create_stub_catalog();
    }

    tracing::info!(tools = catalog.tool_count(), "Tool catalog loaded");

    let id_remapper = IdRemapper::new();

    // JWT authentication: validate token at session start
    let caller: Option<CallerIdentity> = {
        let secret = std::env::var(&config.auth.jwt_secret_env).unwrap_or_default();
        if secret.is_empty() {
            tracing::warn!(
                env_var = %config.auth.jwt_secret_env,
                "JWT secret not set, auth disabled (dev mode)"
            );
            None
        } else {
            let validator = JwtValidator::new(
                secret.as_bytes(),
                &config.auth.jwt_issuer,
                &config.auth.jwt_audience,
            );
            let token = std::env::var("SENTINEL_TOKEN").map_err(|_| {
                anyhow::anyhow!("SENTINEL_TOKEN env var required when JWT auth is enabled")
            })?;
            let identity = validator.validate(&token).map_err(|e| {
                anyhow::anyhow!("Authentication failed: {e}")
            })?;
            tracing::info!(
                subject = %identity.subject,
                role = %identity.role,
                "Session authenticated"
            );
            Some(identity)
        }
    };

    let (inbound_tx, inbound_rx) = mpsc::channel::<String>(64);
    let (outbound_tx, outbound_rx) = mpsc::channel::<String>(64);

    tokio::spawn(sentinel_gateway::transport::stdio::stdio_reader(inbound_tx));
    tokio::spawn(sentinel_gateway::transport::stdio::stdio_writer(outbound_rx));

    sentinel_gateway::gateway::run_dispatch(
        inbound_rx,
        outbound_tx,
        &catalog,
        &backends_map,
        &id_remapper,
        caller,
        &config.rbac,
    )
    .await?;

    tracing::info!("Dispatch loop ended (stdin closed)");
    Ok(())
}
