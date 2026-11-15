//! How one action write is committed: the guarded transition, its audit and journal rows,
//! and whatever else travels in the same transaction.

use domovoi_core::ActionRequest;
use domovoi_core::ports::{
    ActionGuard, AggregateRef, AuditDraft, CommitError, JournalDraft, JournalKind, StateWrite,
    Transaction,
};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{EventKind, Publish, Shared};

/// The journal row one action transition writes. `extra` adds one field, such as a
/// refusal's reason.
fn journal_for(
    action: &ActionRequest,
    guard: &ActionGuard,
    audit: &AuditDraft,
    now: Timestamp,
    extra: Option<(&str, &str)>,
) -> JournalDraft {
    let from = match guard {
        ActionGuard::New => None,
        ActionGuard::Status(status) => Some(status.as_str()),
    };
    let mut payload = json!({
        "action": action.id().to_string(),
        "incident": action.incident().to_string(),
        "host": action.host().to_string(),
        "skill": action.skill().as_str(),
        "from": from,
        "to": action.status().name(),
        "plan_hash": action.plan_hash().to_string(),
    });
    if let (Value::Object(fields), Some((key, value))) = (&mut payload, extra) {
        fields.insert(key.to_owned(), json!(value));
    }
    JournalDraft {
        aggregate: AggregateRef::action(action.id()),
        kind: JournalKind::ActionTransition,
        payload_v: 1,
        payload,
        actor: audit.actor.clone(),
        occurred_at: now,
    }
}

impl Shared {
    /// Commits one action write together with its audit and journal entries, then records
    /// the status-change metric (only when the status actually changed) and publishes the
    /// event. Nothing is recorded or published when the commit fails, so a store error can
    /// never leave an action in a new status without its audit and journal rows. The
    /// journal's `from` is the status the guard names; it is `null` for a creation.
    pub(crate) async fn commit_action(
        &self,
        action: &ActionRequest,
        guard: ActionGuard,
        audit: AuditDraft,
        now: Timestamp,
        extra: Option<(&str, &str)>,
        publish: Publish,
    ) -> Result<(), CommitError> {
        let journal = journal_for(action, &guard, &audit, now, extra);
        self.commit_built(action, guard, audit, journal, publish, |tx| tx)
            .await
    }

    /// [`Shared::commit_action`] with a hook that adds effects to the same transaction, so an
    /// effect is created or completed only if the transition and its rows commit.
    pub(crate) async fn commit_action_with<F>(
        &self,
        action: &ActionRequest,
        guard: ActionGuard,
        audit: AuditDraft,
        now: Timestamp,
        publish: Publish,
        with: F,
    ) -> Result<(), CommitError>
    where
        F: FnOnce(Transaction) -> Transaction,
    {
        let journal = journal_for(action, &guard, &audit, now, None);
        self.commit_built(action, guard, audit, journal, publish, with)
            .await
    }

    /// The one place an action transition is committed: the guarded write, its audit and
    /// journal rows, whatever `with` adds, and then the metric and the event.
    async fn commit_built<F>(
        &self,
        action: &ActionRequest,
        guard: ActionGuard,
        audit: AuditDraft,
        journal: JournalDraft,
        publish: Publish,
        with: F,
    ) -> Result<(), CommitError>
    where
        F: FnOnce(Transaction) -> Transaction,
    {
        let changed = match &guard {
            ActionGuard::New => true,
            ActionGuard::Status(status) => status.as_str() != action.status().name(),
        };
        let tx = Transaction::new()
            .write(StateWrite::Action {
                action: action.clone(),
                expected: guard,
            })
            .audit(audit)
            .journal(journal);
        self.deps.uow.commit(with(tx)).await?;
        if changed {
            crate::telemetry::record_action(action.status());
        }
        if publish == Publish::ActionChanged {
            self.publish(
                action,
                EventKind::ActionChanged {
                    action: action.id(),
                    status: action.status().clone(),
                },
            )
            .await;
        }
        Ok(())
    }

    /// Commits one transaction that is not an action transition, such as a fenced effect
    /// completion.
    pub(crate) async fn commit_tx(&self, tx: Transaction) -> Result<(), CommitError> {
        self.deps.uow.commit(tx).await.map(|_| ())
    }
}
