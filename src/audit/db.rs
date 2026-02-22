use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub request_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub client_subject: String,
    pub client_role: String,
    pub tool_name: String,
    pub backend_name: String,
    pub request_args: Option<Value>,
    pub response_status: String,
    pub error_message: Option<String>,
    pub latency_ms: i64,
}

pub async fn create_pool(url: &str, max_connections: u32) -> Result<sqlx::PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
}

pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    tracing::info!("Database migrations complete");
    Ok(())
}

pub async fn insert_audit_entry(pool: &sqlx::PgPool, entry: &AuditEntry) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO audit_log
           (request_id, timestamp, client_subject, client_role, tool_name,
            backend_name, request_args, response_status, error_message, latency_ms)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(entry.request_id)
    .bind(entry.timestamp)
    .bind(&entry.client_subject)
    .bind(&entry.client_role)
    .bind(&entry.tool_name)
    .bind(&entry.backend_name)
    .bind(&entry.request_args)
    .bind(&entry.response_status)
    .bind(&entry.error_message)
    .bind(entry.latency_ms)
    .execute(pool)
    .await?;
    Ok(())
}
