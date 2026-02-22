use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::backend::HttpBackend;

use super::server::{BackendHealth, BackendHealthMap};

pub async fn health_checker(
    backends: Vec<(String, HttpBackend)>,
    health_map: BackendHealthMap,
    cancel: CancellationToken,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    let ping_body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                for (name, backend) in &backends {
                    let result = backend.send(ping_body).await;
                    let mut map = health_map.write().await;
                    let entry = map.entry(name.clone()).or_insert(BackendHealth {
                        healthy: false,
                        last_check: Instant::now(),
                        consecutive_failures: 0,
                    });
                    match result {
                        Ok(_) => {
                            entry.healthy = true;
                            entry.consecutive_failures = 0;
                            entry.last_check = Instant::now();
                            tracing::debug!(backend = %name, "Health check passed");
                        }
                        Err(e) => {
                            entry.healthy = false;
                            entry.consecutive_failures += 1;
                            entry.last_check = Instant::now();
                            tracing::warn!(
                                backend = %name,
                                failures = entry.consecutive_failures,
                                error = %e,
                                "Health check failed"
                            );
                        }
                    }
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Health checker shutting down");
                break;
            }
        }
    }
}
