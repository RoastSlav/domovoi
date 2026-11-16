//! Recomputing a v2 approval hash from one coherent agent-session snapshot.

use domovoi_core::{
    ActionRequest, ExecutionId, ImplHash, Incident, KeyFingerprint, PlanContract, PlanHash,
    PlanTarget, SkillSummary, ValidatedArgs,
};

use super::Shared;
use super::preflight;

/// Recomputes the v2 approval hash for `action` from what the hub has now: the agent's
/// current enrolled key, the incident's subject, the current manifest and entry digest, the
/// arguments and the action's execution identity.
///
/// The key is re-read, never remembered: re-enrolment between approval and dispatch must
/// invalidate the approval. No live agent means no canonical host identity, so the action is
/// rejected on the stale path instead of hashing a placeholder.
pub(super) async fn v2_plan(
    shared: &Shared,
    action: &ActionRequest,
    skill: &SkillSummary,
    args: &ValidatedArgs,
    incident: &Incident,
    snapshot: &domovoi_core::ports::AgentSnapshot,
) -> Option<(PlanHash, PlanTarget)> {
    let agent = match shared.deps.hosts.live_agent(action.host()).await {
        Ok(Some(agent)) => agent,
        Ok(None) => {
            let _ = preflight::ask_reject(
                shared,
                action.id(),
                "the host has no enrolled agent key; the plan can no longer be verified".to_owned(),
                false,
            )
            .await;
            return None;
        }
        Err(err) => {
            tracing::error!(
                %err,
                action = %action.id(),
                "could not read the host's enrolled agent; the action stays approved",
            );
            return None;
        }
    };
    // The key and skills must belong to the session that announced them.
    if agent.agent != snapshot.agent {
        let _ = preflight::ask_reject(
            shared,
            action.id(),
            "the host's enrolled agent changed before dispatch".to_owned(),
            false,
        )
        .await;
        return None;
    }
    let target =
        PlanTarget::of_incident(incident, KeyFingerprint::of_public_key(&agent.public_key));
    let contract = match PlanContract::skill(action.skill(), 1) {
        Ok(contract) => contract,
        Err(err) => {
            let _ = preflight::ask_reject(shared, action.id(), err.to_string(), false).await;
            return None;
        }
    };
    let hash = PlanHash::compute_v2(
        &contract,
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &target,
        args,
        ExecutionId::from_action(action.id()),
    );
    Some((hash, target))
}
