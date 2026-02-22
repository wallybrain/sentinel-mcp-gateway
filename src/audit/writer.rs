use super::db::{insert_audit_entry, AuditEntry};
use tokio::sync::mpsc;

pub async fn audit_writer(pool: sqlx::PgPool, mut rx: mpsc::Receiver<AuditEntry>) {
    while let Some(entry) = rx.recv().await {
        if let Err(e) = insert_audit_entry(&pool, &entry).await {
            tracing::error!(error = %e, "Failed to write audit log");
        }
    }

    // Drain any remaining buffered entries before exiting
    while let Ok(entry) = rx.try_recv() {
        if let Err(e) = insert_audit_entry(&pool, &entry).await {
            tracing::error!(error = %e, "Failed to write audit log during drain");
        }
    }

    tracing::info!("Audit writer shutting down");
}
