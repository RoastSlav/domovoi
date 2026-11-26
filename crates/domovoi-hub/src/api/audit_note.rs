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
