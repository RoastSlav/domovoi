//! Proposing an execution: the plan a person will approve and dispatch will recompute.
//!
//! The v2 plan hash binds the contract and its version, the implementation (manifest and
//! entry script), the target (host, its enrolled key, check and target), the validated
//! arguments and the execution's own id, which is its step id.

use domovoi_core::capability::TargetQuery;
use domovoi_core::ports::AgentSnapshot;
use domovoi_core::{
    ExecutionId, ImplHash, PlanContract, PlanHash, PlanTarget, Risk, SkillSummary, StepId,
    ValidatedArgs,
};
use jiff::Timestamp;

use super::decide::ExecutionProposal;
use super::error::OperationError;
use super::handle::ExecutionRequest;
use super::writer::OpShared;

/// The v2 plan hash of running `skill` as Execution step `step`.
///
/// The one place the hash is computed: a proposal stores it and the dispatch recomputes it
/// with the same inputs read again.
pub(crate) fn plan_hash(
    skill: &SkillSummary,
    contract_version: u32,
    target: &PlanTarget,
    args: &ValidatedArgs,
    step: StepId,
) -> Result<PlanHash, String> {
    let contract = PlanContract::skill(&skill.name, contract_version).map_err(|e| e.to_string())?;
    Ok(PlanHash::compute_v2(
        &contract,
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        target,
        args,
        ExecutionId::from_step(step),
    ))
}

/// Builds the proposal for `request`: reads the live session, finds and validates the skill
/// and computes the plan hash. A request that cannot be run comes back as a proposal with a
/// `refused` reason, so the step is stored refused; a host with no live session is an error
/// and nothing is stored.
pub(super) async fn propose(
    shared: &OpShared,
    request: ExecutionRequest,
) -> Result<(StepId, ExecutionProposal), OperationError> {
    let ExecutionRequest {
        intent,
        parent,
        caused_by,
    } = request;
    let host = intent.host;
    let Some(session) = shared.deps.observer.observe_snapshot(host) else {
        return Err(OperationError::HostUnavailable(host));
    };
    let step = StepId::new();
    let expires_at = shared
        .deps
        .clock
        .now()
        .checked_add(shared.settings.approval_ttl)
        .unwrap_or(Timestamp::MAX);
    let hash = hash_for(shared, &intent, step, &session).await;
    let (plan_hash, risk, refused) = match hash {
        Ok((hash, risk)) => (Some(hash), risk, None),
        Err(reason) => (None, Risk::Destructive, Some(reason)),
    };
    let proposal = ExecutionProposal {
        intent,
        step,
        approval_step: StepId::new(),
        parent,
        caused_by,
        plan_hash,
        expires_at,
        risk,
        refused,
    };
    Ok((step, proposal))
}

/// The plan hash and declared risk of `intent` against the skills `session` announced, or
/// why it cannot run.
async fn hash_for(
    shared: &OpShared,
    intent: &domovoi_core::ExecutionIntent,
    step: StepId,
    session: &AgentSnapshot,
) -> Result<(PlanHash, Risk), String> {
    let Some(name) = intent.contract.skill_name() else {
        return Err("only skill contracts can be run".to_owned());
    };
    let Some(skill) = session.skills.iter().find(|skill| skill.name == name) else {
        return Err("the agent does not offer this skill".to_owned());
    };
    let args = skill
        .validate_args(&intent.args)
        .map_err(|err| err.to_string())?;
    let resolved = shared
        .deps
        .resolver
        .resolve(TargetQuery::Host { host: intent.host })
        .await
        .map_err(|err| err.to_string())?;
    let target = PlanTarget::new(
        intent.host,
        resolved.host_key,
        intent.check.clone(),
        intent.target.clone(),
    );
    let hash = plan_hash(skill, intent.contract_version, &target, &args, step)?;
    Ok((hash, skill.risk))
}
