use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::metrics::Metrics;

pub struct BackendHealth {
    pub healthy: bool,
    pub last_check: Instant,
    pub consecutive_failures: u32,
}

pub type BackendHealthMap = Arc<RwLock<HashMap<String, BackendHealth>>>;

#[derive(Clone)]
pub struct HealthAppState {
    pub health_map: BackendHealthMap,
    pub metrics: Option<Arc<Metrics>>,
}

async fn liveness() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn readiness(State(state): State<HealthAppState>) -> (StatusCode, Json<Value>) {
    let map = state.health_map.read().await;
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

async fn metrics_handler(State(state): State<HealthAppState>) -> impl IntoResponse {
    match &state.metrics {
        Some(m) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            m.gather_text(),
        ),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Metrics not enabled".to_string(),
        ),
    }
}

pub fn build_health_router(
    health_map: BackendHealthMap,
    metrics: Option<Arc<Metrics>>,
) -> Router {
    let state = HealthAppState {
        health_map,
        metrics,
    };
    Router::new()
        .route("/health", get(liveness))
        .route("/ready", get(readiness))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

pub async fn run_health_server(
    addr: &str,
    health_map: BackendHealthMap,
    metrics: Option<Arc<Metrics>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let app = build_health_router(health_map, metrics);

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
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn make_health_map() -> BackendHealthMap {
        Arc::new(RwLock::new(HashMap::new()))
    }

    fn build_app(health_map: BackendHealthMap) -> Router {
        build_health_router(health_map, None)
    }

    #[tokio::test]
    async fn liveness_returns_200() {
        let app = build_app(make_health_map());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_503_on_empty_map() {
        let app = build_app(make_health_map());
        let req = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn readiness_200_when_backend_healthy() {
        let map = make_health_map();
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
        let map = make_health_map();
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

    #[tokio::test]
    async fn test_metrics_endpoint_returns_prometheus_text() {
        let metrics = Arc::new(Metrics::new());
        metrics.record_request("echo", "success", 0.01);
        let app = build_health_router(make_health_map(), Some(metrics));
        let req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            text.contains("sentinel_requests_total"),
            "Expected prometheus metrics in body"
        );
    }

    #[tokio::test]
    async fn test_metrics_endpoint_returns_404_when_disabled() {
        let app = build_health_router(make_health_map(), None);
        let req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
