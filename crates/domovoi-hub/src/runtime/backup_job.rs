//! Recording how the nightly backup went: the advisory and the success gauge.

use domovoi_core::ports::{AdvisoryRepo, Redactor, StoreError};
use domovoi_core::{Advisory, AdvisoryKind};
use domovoi_engine::advisories::advisory_id;
use jiff::Timestamp;

/// The id of the nightly backup advisory, the one place it is raised and cleared.
fn backup_advisory_id() -> domovoi_core::AdvisoryId {
    advisory_id(AdvisoryKind::BackupFailed, None, "nightly")
}

/// Writes the audit row of one backup run: `backup.created` with the file name and size, or
/// `backup.failed`. The reason text is not stored here (it is in the advisory, redacted).
pub(crate) async fn audit_backup(
    audit: &dyn domovoi_core::ports::AuditLog,
    now: Timestamp,
    result: &Result<std::path::PathBuf, domovoi_store::StoreOpenError>,
) {
    let (kind, detail) = match result {
        Ok(path) => (
            "backup.created",
            serde_json::json!({
                "file": path.file_name().map(|n| n.to_string_lossy().into_owned()),
                "size_bytes": tokio::fs::metadata(path).await.map(|m| m.len()).ok(),
            }),
        ),
        Err(_) => ("backup.failed", serde_json::Value::Null),
    };
    let event = domovoi_core::ports::AuditEvent {
        at: now,
        actor: "system:backups".to_owned(),
        kind: kind.to_owned(),
        subject: "hub.db".to_owned(),
        detail,
    };
    if let Err(err) = audit.append(&event).await {
        tracing::error!(%err, kind, "could not append an audit row");
    }
}

/// Records the outcome of the nightly backup: a failure raises `backup_failed` and says
/// the newest earlier copy is still there; a success clears it and sets the "last good
/// backup" gauge.
pub(crate) async fn record_backup_result(
    store: &dyn AdvisoryRepo,
    redactor: &dyn Redactor,
    now: Timestamp,
    result: Result<std::path::PathBuf, domovoi_store::StoreOpenError>,
) {
    match result {
        Ok(path) => {
            let size = tokio::fs::metadata(&path).await.map(|m| m.len()).ok();
            tracing::info!(
                path = %path.display(),
                size_bytes = size,
                "nightly backup written"
            );
            metrics::gauge!("domovoi_backup_last_success_timestamp_seconds")
                .set(now.as_second() as f64);
            match store.resolve(backup_advisory_id(), now).await {
                Ok(()) | Err(StoreError::NotFound) => {}
                Err(err) => tracing::warn!(%err, "could not clear the backup advisory"),
            }
        }
        Err(err) => {
            // The error text carries paths and whatever SQLite or the file system said,
            // so it is redacted before it is logged or stored.
            let reason = redactor.redact(&err.to_string());
            tracing::warn!(reason = %reason, "nightly backup failed");
            let advisory = Advisory {
                id: backup_advisory_id(),
                kind: AdvisoryKind::BackupFailed,
                host: None,
                title: "The nightly backup failed".to_owned(),
                detail: format!(
                    "{reason}. The newest earlier backup is still in the backups folder."
                ),
                created_at: now,
                resolved_at: None,
            };
            if let Err(err) = store.upsert(&advisory).await {
                tracing::warn!(%err, "could not record the backup advisory");
            }
        }
    }
}
