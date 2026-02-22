use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::sync::{mpsc, RwLock};
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;

use sentinel_gateway::audit;
use sentinel_gateway::auth::jwt::{CallerIdentity, JwtValidator};
use sentinel_gateway::backend::{build_http_client, discover_tools, Backend, HttpBackend};
use sentinel_gateway::config::types::BackendType;
use sentinel_gateway::health::checker::health_checker;
use sentinel_gateway::health::circuit_breaker::CircuitBreaker;
use sentinel_gateway::health::server::{run_health_server, BackendHealth, BackendHealthMap};
use sentinel_gateway::protocol::id_remapper::IdRemapper;
use sentinel_gateway::ratelimit::RateLimiter;

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
    let mut backends_map: HashMap<String, Backend> = HashMap::new();
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
                            backends_map.insert(backend_config.name.clone(), Backend::Http(backend));
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

    // Audit logging initialization
    let (audit_tx, audit_handle) = if config.gateway.audit_enabled {
        match std::env::var(&config.postgres.url_env) {
            Ok(url) if !url.is_empty() => {
                let pool = audit::db::create_pool(&url, config.postgres.max_connections).await?;
                audit::db::run_migrations(&pool).await?;
                let (atx, arx) = mpsc::channel::<audit::db::AuditEntry>(1024);
                let handle = tokio::spawn(audit::writer::audit_writer(pool, arx));
                tracing::info!("Audit logging enabled (Postgres)");
                (Some(atx), Some(handle))
            }
            _ => {
                tracing::warn!(
                    env_var = %config.postgres.url_env,
                    "Postgres URL not set, audit logging disabled"
                );
                (None, None)
            }
        }
    } else {
        tracing::info!("Audit logging disabled");
        (None, None)
    };

    let rate_limiter = RateLimiter::new(&config.rate_limits);

    // Initialize shared health state (optimistic start: all discovered backends healthy)
    let health_map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
    {
        let mut map = health_map.write().await;
        for name in backends_map.keys() {
            map.insert(
                name.clone(),
                BackendHealth {
                    healthy: true,
                    last_check: std::time::Instant::now(),
                    consecutive_failures: 0,
                },
            );
        }
    }

    // Create per-backend circuit breakers
    let circuit_breakers: HashMap<String, CircuitBreaker> = config
        .backends
        .iter()
        .filter(|b| backends_map.contains_key(&b.name))
        .map(|b| {
            (
                b.name.clone(),
                CircuitBreaker::new(
                    b.circuit_breaker_threshold,
                    Duration::from_secs(b.circuit_breaker_recovery_secs),
                ),
            )
        })
        .collect();

    // Create CancellationToken for graceful shutdown
    let cancel = CancellationToken::new();

    // Spawn signal handler
    let cancel_signal = cancel.clone();
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
            _ = sigint.recv() => tracing::info!("Received SIGINT"),
        }
        cancel_signal.cancel();
    });

    // Spawn health HTTP server
    let health_addr = config.gateway.health_listen.clone();
    let health_map_server = health_map.clone();
    let cancel_server = cancel.clone();
    tokio::spawn(async move {
        if let Err(e) = run_health_server(&health_addr, health_map_server, cancel_server).await {
            tracing::error!(error = %e, "Health server exited with error");
        }
    });

    // Spawn health checker
    let backends_list: Vec<(String, HttpBackend)> = backends_map
        .iter()
        .filter_map(|(name, backend)| match backend {
            Backend::Http(h) => Some((name.clone(), h.clone())),
            _ => None,
        })
        .collect();
    tokio::spawn(health_checker(
        backends_list,
        health_map.clone(),
        cancel.clone(),
        30,
    ));

    let (inbound_tx, inbound_rx) = mpsc::channel::<String>(64);
    let (outbound_tx, outbound_rx) = mpsc::channel::<String>(64);

    tokio::spawn(sentinel_gateway::transport::stdio::stdio_reader(inbound_tx));
    tokio::spawn(sentinel_gateway::transport::stdio::stdio_writer(outbound_rx));

    let dispatch_audit_tx = audit_tx.clone();
    sentinel_gateway::gateway::run_dispatch(
        inbound_rx,
        outbound_tx,
        &catalog,
        &backends_map,
        &id_remapper,
        caller,
        &config.rbac,
        dispatch_audit_tx,
        &rate_limiter,
        &config.kill_switch,
        &circuit_breakers,
        cancel.clone(),
    )
    .await?;

    // Ordered shutdown sequence
    cancel.cancel();

    // Drop audit_tx to signal the writer to drain
    drop(audit_tx);

    // Wait for audit writer to finish draining
    if let Some(handle) = audit_handle {
        let _ = handle.await;
    }

    tracing::info!("Shutdown complete");
    Ok(())
}
