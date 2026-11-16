//! The writer's dispatch decision: prove the authorization, re-check the target and session,
//! then record the transition in the same guarded commit as every other decision.

use super::approvals::load;
use super::freshness;
use super::writer::{Decision, stale, transition};
use super::{ApprovalError, Shared, audit, expire, reject};
use domovoi_core::{
    ActionChange, ActionId, ActionRequest, ActionStatus, AgentSessionId, KeyFingerprint,
    PlanFamily, PlanHash, PlanTarget,
};

/// Sends `id` to the agent: `Approved → Dispatched` when the plan the agent has now
/// still matches the approved one, `Approved → Rejected` when it does not (INV-4), and
/// `Approved → NotRun` when the approval window closed while the request waited. The audit
/// line says which happened, and the caller only runs the skill when the action it gets
/// back is `Dispatched`. The run records the journal epoch of the authenticated session the
/// plan was calculated from, so a later reconciliation can tell whether that journal is
/// still the one it was sent to.
pub(crate) async fn dispatch(
    shared: &Shared,
    id: ActionId,
    plan: PlanHash,
    target: Option<PlanTarget>,
    session: AgentSessionId,
    epoch: domovoi_core::JournalEpoch,
) -> Decision {
    let mut action = load(shared, id).await?;
    // The stored status says `Approved`; the writer still proves that a human authorized it.
    check_authorization(&action)?;
    let now = shared.deps.clock.now();
    // The executor asked before sending this, but the writer is the one that decides: an
    // approval that expired, or an incident that was deleted or resolved while the
    // request waited in the queue, must not be dispatched either. A store that cannot
    // answer leaves the action approved rather than guessing.
    let fresh = freshness::check(shared, &action, now)
        .await
        .map_err(|err| ApprovalError::Store(err.to_string()))?;
    let incident = match fresh {
        freshness::Fresh::Current(incident) => *incident,
        freshness::Fresh::Stale(old) => {
            if old == freshness::Stale::Expired {
                return expire::approved_loaded(shared, id, &mut action, now).await;
            }
            let reason = old.reason().to_owned();
            return reject::stop(shared, id, &mut action, reason, false, now).await;
        }
    };
    // A replacement session must not receive a plan calculated for the old one, even when
    // the journal epoch survived the reconnection.
    let Some(snapshot) = shared.deps.link.snapshot(action.host()) else {
        let reason = "the agent session changed before dispatch".to_owned();
        return reject::stop(shared, id, &mut action, reason, false, now).await;
    };
    if snapshot.session != session || snapshot.journal_epoch != epoch {
        let reason = "the agent session changed before dispatch".to_owned();
        return reject::stop(shared, id, &mut action, reason, false, now).await;
    }
    // For v2 the target the executor hashed must still be the one the store and the
    // enrolled key describe now, not just the same host.
    if action.approval_plan().family() == PlanFamily::V2 {
        let Some(expected) = target else {
            return reject::stop(
                shared,
                id,
                &mut action,
                "the plan changed after approval".to_owned(),
                false,
                now,
            )
            .await;
        };
        let agent = shared
            .deps
            .hosts
            .live_agent(action.host())
            .await
            .map_err(|err| ApprovalError::Store(err.to_string()))?;
        let Some(agent) = agent else {
            return reject::stop(
                shared,
                id,
                &mut action,
                "the plan changed after approval".to_owned(),
                false,
                now,
            )
            .await;
        };
        let current =
            PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&agent.public_key));
        if current != expected {
            return reject::stop(
                shared,
                id,
                &mut action,
                "the plan changed after approval".to_owned(),
                false,
                now,
            )
            .await;
        }
    }
    let ActionChange { from, .. } = action
        .dispatch(plan, Some(epoch), now)
        .map_err(ApprovalError::Refused)?;
    let entry = match action.status().clone() {
        ActionStatus::Rejected { reason } => audit::rejected(&action, &reason, now),
        _ => audit::dispatched(&action, now),
    };
    transition(shared, &action, &from, entry, now)
        .await
        .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Proves that the stored approval evidence is a human authorization for this plan.
///
/// The discriminator alone is not enough: the human approval and, for v2, the displayed
/// hash and its provenance must agree with the plan. Legacy v1 rows may legitimately lack
/// the displayed hash and provenance, as long as their human approval is intact.
fn check_authorization(action: &ActionRequest) -> Result<(), ApprovalError> {
    match action.authorization_kind() {
        Some(domovoi_core::AuthorizationKind::Human) => {}
        Some(domovoi_core::AuthorizationKind::Policy) => {
            return Err(ApprovalError::NotYetSupported);
        }
        None => return Err(ApprovalError::MissingAuthorization),
    }
    if action.approved_by().is_none() {
        return Err(ApprovalError::MissingAuthorization);
    }
    let plan = action.approval_plan();
    match plan.family() {
        PlanFamily::V2 => {
            if action.approved_plan_hash() != Some(plan.hash()) || action.provenance().is_none() {
                return Err(ApprovalError::InconsistentAuthorization);
            }
        }
        PlanFamily::V1 => {
            if action
                .approved_plan_hash()
                .is_some_and(|approved| approved != plan.hash())
            {
                return Err(ApprovalError::InconsistentAuthorization);
            }
        }
    }
    Ok(())
}
