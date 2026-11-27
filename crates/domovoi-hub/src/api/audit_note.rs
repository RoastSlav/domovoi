//! Best-effort audit rows for web actions that the services do not audit themselves.

use domovoi_core::ports::AuditEvent;
use serde_json::Value;

use super::ApiState;

/// Appends one audit row. The action it records already happened, so a failed append is
/// logged and never turns the action into an error.
pub(super) async fn note(
    state: &ApiState,
    actor: String,
    kind: &str,
    subject: String,
    detail: Value,
) {
    let event = AuditEvent {
        at: state.deps.clock.now(),
        actor,
        kind: kind.to_owned(),
        subject,
        detail,
    };
    if let Err(err) = state.deps.audit.append(&event).await {
        tracing::error!(%err, kind, "could not append an audit row");
    }
}

/// Commits one owner decision together with its audit row: both are stored or neither is.
pub(super) async fn decide(
    state: &ApiState,
    write: domovoi_core::ports::AdminWrite,
    actor: domovoi_core::UserId,
    kind: &str,
    subject: String,
    detail: Value,
) -> Result<(), super::ApiError> {
    use domovoi_core::ports::{AuditDraft, StateWrite, Transaction};
    let draft = AuditDraft::new(
        domovoi_core::Actor::User(actor),
        state.deps.clock.now(),
        kind.to_owned(),
        subject,
        detail,
    );
    state
        .deps
        .uow
        .commit(
            Transaction::new()
                .write(StateWrite::Admin(write))
                .audit(draft),
        )
        .await
        .map(|_| ())
        .map_err(|err| match err {
            domovoi_core::ports::CommitError::Constraint(_) => super::ApiError::NotFound,
            other => super::ApiError::internal("committing a decision", other),
        })
}
