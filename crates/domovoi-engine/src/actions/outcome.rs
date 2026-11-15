//! The outcome commands: what the agent's run produced, and what the verification re-check
//! said. Both are applied by the writer loop, and both commit their effects with the
//! transition.

use domovoi_core::ports::{
    AggregateRef, ClaimToken, CommitError, EffectCompletion, EffectDraft, EffectKind, Transaction,
};
use domovoi_core::{ActionChange, ActionError, ActionId, ActionStatus, EffectId};
use jiff::SignedDuration;
use serde_json::json;

use super::approvals::load;
use super::audit::{self, RunRecord};
use super::writer::{Decision, stale, transition_with};
use super::{ApprovalError, Shared};

/// The agent's answer for a run: `Dispatched → Succeeded` or `Dispatched → Failed`.
/// `record` is what the run produced, `None` when the skill never ran. A successful run that
/// owes a re-check also creates the durable `wake` effect in the same commit, so the
/// verification cannot be lost by a restart or a failed write.
pub(super) async fn complete(
    shared: &Shared,
    id: ActionId,
    outcome: Result<(), String>,
    record: Option<RunRecord>,
    verify_after: Option<SignedDuration>,
) -> Decision {
    let succeeded = outcome.is_ok();
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action
        .complete(outcome, now)
        .map_err(ApprovalError::Refused)?;
    let entry = match action.status().clone() {
        ActionStatus::Failed { reason } => {
            audit::finished(&action, record.as_ref(), Some(&reason), now)
        }
        _ => audit::finished(&action, record.as_ref(), None, now),
    };
    let effect = if succeeded {
        verify_after.map(|delay| EffectDraft {
            id: EffectId::new(),
            kind: EffectKind::Wake,
            aggregate: AggregateRef::action(id),
            payload: json!({"v": 1, "purpose": "verify_action", "action": id.to_string()}),
            not_before: now.checked_add(delay).unwrap_or(now),
            max_attempts: 5,
            dedup_key: Some(format!("verify:{id}")),
            created_at: now,
        })
    } else {
        None
    };
    let created = effect.is_some();
    transition_with(shared, &action, &from, entry, now, |tx| match effect {
        Some(draft) => tx.effect(draft),
        None => tx,
    })
    .await
    .map_err(|err| stale(err, id))?;
    if created {
        shared.effects_nudge.notify_one();
    }
    Ok(action)
}

/// The re-check of the originating check after a successful run: `Succeeded → Verified`
/// or `Succeeded → VerifyFailed`. `summary` is what the re-check said, kept in the audit
/// line so the person can see why the incident is fixed. When the answer belongs to a `wake`
/// effect, its completion is committed with the transition, fenced by the claim token.
pub(super) async fn verify(
    shared: &Shared,
    id: ActionId,
    passed: bool,
    summary: String,
    effect: Option<(EffectId, ClaimToken)>,
) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action
        .verified(passed, now)
        .map_err(ApprovalError::Refused)?;
    let completion = effect.map(|(id, token)| EffectCompletion {
        id,
        claim_token: Some(token),
        at: now,
    });
    transition_with(
        shared,
        &action,
        &from,
        audit::verification(&action, passed, &summary, now),
        now,
        |tx| match completion {
            Some(completion) => tx.complete(completion),
            None => tx,
        },
    )
    .await
    .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Marks a wake effect done without changing any state, for one that has nothing left to
/// verify. The completion is still fenced by the claim token, so a stale holder changes
/// nothing.
pub(super) async fn finish_effect(
    shared: &Shared,
    effect: EffectId,
    claim: ClaimToken,
) -> Result<(), ApprovalError> {
    let now = shared.deps.clock.now();
    let tx = Transaction::new().complete(EffectCompletion {
        id: effect,
        claim_token: Some(claim),
        at: now,
    });
    shared.commit_tx(tx).await.map_err(|err| match err {
        CommitError::StaleEffect { .. } => ApprovalError::StaleEffect,
        CommitError::Constraint(message) | CommitError::Backend(message) => {
            ApprovalError::Store(message)
        }
        CommitError::Conflict { .. } => ApprovalError::Refused(ActionError::ChangedMeanwhile),
    })
}
