use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub struct BackendHealth {
    pub healthy: bool,
    pub last_check: Instant,
    pub consecutive_failures: u32,
}

pub type BackendHealthMap = Arc<RwLock<HashMap<String, BackendHealth>>>;

async fn liveness() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn readiness(
    State(health_map): State<BackendHealthMap>,
) -> (StatusCode, Json<Value>) {
    let map = health_map.read().await;
    if map.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "not_ready", "reason": "no backends registered"})),
        );
    }
    if map.values().any(|h| h.healthy) {
        (StatusCode::OK, Json(json!({"status": "ready"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "not_ready"})),
        )
    }
}

pub fn build_health_router(health_map: BackendHealthMap) -> Router {
    Router::new()
        .route("/health", get(liveness))
        .route("/ready", get(readiness))
        .with_state(health_map)
}

pub async fn run_health_server(
    addr: &str,
    health_map: BackendHealthMap,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let app = build_health_router(health_map);

    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = %addr, "Health server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn build_app(health_map: BackendHealthMap) -> Router {
        Router::new()
            .route("/health", get(liveness))
            .route("/ready", get(readiness))
            .with_state(health_map)
    }

    #[tokio::test]
    async fn liveness_returns_200() {
        let map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
        let app = build_app(map);
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_503_on_empty_map() {
        let map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
        let app = build_app(map);
        let req = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn readiness_200_when_backend_healthy() {
        let map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
        map.write().await.insert(
            "test-backend".to_string(),
            BackendHealth {
                healthy: true,
                last_check: Instant::now(),
                consecutive_failures: 0,
            },
        );
        let app = build_app(map);
        let req = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_503_when_all_unhealthy() {
        let map: BackendHealthMap = Arc::new(RwLock::new(HashMap::new()));
        map.write().await.insert(
            "test-backend".to_string(),
            BackendHealth {
                healthy: false,
                last_check: Instant::now(),
                consecutive_failures: 3,
            },
        );
        let app = build_app(map);
        let req = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
