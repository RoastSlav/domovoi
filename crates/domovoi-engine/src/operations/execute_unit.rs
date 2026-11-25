//! The dispatch of an authorized `unit.restart`: the one engine file that hands a restart to a
//! host's agent.
//!
//! It takes the same [`AuthorizedStep`] the skill path takes, so only a row that carries a person's
//! approval or a stored permit can reach it. Before anything is sent it checks what only the hub can
//! check (the window, the pause switch, an unresolved unknown outcome, the loop guard), resolves
//! the unit again through the helper and recomputes the plan hash, so a configuration change since
//! approval is a refusal and never a restart. The agent and the helper check their own limits again.
//! Nothing is retried after the hand-off: a result that does not arrive leaves the step dispatched
//! until the agent's journal can settle it.

use domovoi_core::capability::{TargetError, TargetName};
use domovoi_core::ports::{AgentSnapshot, LinkError};
use domovoi_core::unit::{UnitCall, UnitDispatch, UnitOutputs, UnitRefusal, keys};
use domovoi_core::{AuthorizationKind, CapabilityPlan, ExecutionId, ExecutionRecord, StepOutcome};
use jiff::SignedDuration;

use super::effects::{Ending, settle};
use super::execute::{AuthorizedStep, ExecuteError, commit_dispatch};
use super::writer::{Held, OpShared, queue_reconcile};

/// The reason for a restart the hub proved it never handed to the agent.
const HANDOFF_FAILED: &str = "hub_not_handed_off";
/// What the restart contract allows one run to take, before the grace the dispatch adds.
const RESTART_TIMEOUT: SignedDuration = SignedDuration::from_secs(60);
/// The refusals the agent answers with before it accepts a request.
const REFUSED_FRAMES: [&str; 8] = [
    "ceiling_exceeded",
    "rate_limited",
    "allow_risk_denied",
    "authorization_expired",
    "plan_hash_mismatch",
    "contract_unknown",
    "contract_version_unsupported",
    "journal_corrupt",
];
/// The longest reason kept for an answer nobody recognised.
const UNRECOGNISED_MAX: usize = 500;

/// How a restart's answer settles its step.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum End {
    /// The step ends with this outcome, reason and output.
    Settle(StepOutcome, Option<String>, Option<String>),
    /// Nothing proves either result: the step stays dispatched and a reconcile is queued.
    Unknown,
}

/// Maps the answer to a restart onto the step's ending, by the fixed table: a verified or
/// unverified success; a refusal before the restart (the agent's, or the helper's pre-restart
/// tokens) is `Refused` and never counts against a rule; a restart that began and did not end well
/// is `Failed`; no answer is unknown.
pub(super) fn end_of(result: Result<UnitOutputs, LinkError>) -> End {
    match result {
        Ok(outputs) => {
            let before = outputs.get(keys::OUT_INVOCATION_BEFORE).unwrap_or_default();
            let after = outputs.get(keys::OUT_INVOCATION_AFTER).unwrap_or_default();
            let text = format!("restarted: invocation {before} -> {after}");
            if outputs.get(keys::OUT_VERIFICATION) == Some(keys::VERIFIED) {
                End::Settle(StepOutcome::Succeeded, None, Some(text))
            } else {
                End::Settle(
                    StepOutcome::Succeeded,
                    Some("restarted; the new process could not be checked".to_owned()),
                    Some(text),
                )
            }
        }
        Err(LinkError::Rejected(message)) => {
            let token = message.split(": ").next().unwrap_or_default();
            if REFUSED_FRAMES.contains(&token) {
                return End::Settle(StepOutcome::Refused, Some(message), None);
            }
            match UnitRefusal::from_message(&message) {
                Some(refusal) if refusal.is_pre_restart() => {
                    End::Settle(StepOutcome::Refused, Some(message), None)
                }
                Some(_) => End::Settle(StepOutcome::Failed, Some(message), None),
                None => End::Settle(
                    StepOutcome::Failed,
                    Some(message.chars().take(UNRECOGNISED_MAX).collect()),
                    None,
                ),
            }
        }
        Err(_) => End::Unknown,
    }
}

/// Refuses the run before it is dispatched, through the writer, and says why.
async fn refuse(
    shared: &OpShared,
    held: Held,
    row: &ExecutionRecord,
    reason: String,
) -> ExecuteError {
    let ending = Ending::new(row.operation, row.step, StepOutcome::Refused)
        .because(reason.clone())
        .holding(held);
    log(settle(shared, ending).await, row);
    ExecuteError::Refused(reason)
}

fn log(result: Result<(), super::OperationError>, row: &ExecutionRecord) {
    if let Err(err) = result {
        tracing::debug!(%err, step = %row.step, "the result of a restart was not recorded");
    }
}

/// Sends one authorized `unit.restart` to its host's agent, or settles the step without sending.
pub(super) async fn execute_unit_restart(
    shared: &OpShared,
    held: Held,
    authorized: AuthorizedStep,
    session: AgentSnapshot,
) -> Result<(), ExecuteError> {
    let row = authorized.execution();
    let (operation, step, host) = (row.operation, row.step, row.host);
    let deps = &shared.deps;
    let Some(autonomy) = &deps.autonomy else {
        let reason = "unit restarts are not set up on this hub".to_owned();
        return Err(refuse(shared, held, row, reason).await);
    };
    let now = deps.clock.now();
    if now >= row.expires_at {
        return Err(ExecuteError::Expired);
    }
    // The gate closes the window between the commit that says dispatched and the frame reaching
    // the agent's queue, and is taken before the loop guard counts.
    let gate = shared.dispatch_gate(host).await;
    let since = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let recent = deps
        .operations
        .dispatched_since(host, &row.contract, since)
        .await
        .map_err(|err| {
            tracing::warn!(%err, %step, "could not count the recent dispatches; not dispatching");
            ExecuteError::NotDispatched
        })?;
    let limit = shared.settings.loop_guard_per_hour;
    if recent >= limit {
        let reason = format!(
            "loop guard: {} already ran {limit} times on this host in the last hour",
            row.contract
        );
        return Err(refuse(shared, held, row, reason).await);
    }
    // What only a permit has to satisfy, read fresh: it must be in date, the autonomy switch must
    // be off, and no earlier restart of this unit may have an unknown outcome nobody resolved.
    if let Some(stored) = row.permit() {
        if now >= stored.permit().expires_at() {
            let ending = Ending::new(operation, step, StepOutcome::NotRun)
                .because("authorization_expired")
                .holding(held);
            log(settle(shared, ending).await, row);
            return Ok(());
        }
        let state = autonomy.permits.autonomy().await.map_err(|err| {
            tracing::warn!(%err, %step, "could not read the pause switch; not dispatching");
            ExecuteError::NotDispatched
        })?;
        if state.paused {
            let ending = Ending::new(operation, step, StepOutcome::NotRun)
                .because("autonomy_paused")
                .holding(held);
            log(settle(shared, ending).await, row);
            return Ok(());
        }
        if let Some(target) = &row.target {
            let unknown = autonomy
                .permits
                .unresolved_unknown(host, &row.contract, target)
                .await
                .map_err(|err| {
                    tracing::warn!(%err, %step, "could not read unknown outcomes; not dispatching");
                    ExecuteError::NotDispatched
                })?;
            if unknown.is_some() {
                let reason = "unresolved_unknown".to_owned();
                return Err(refuse(shared, held, row, reason).await);
            }
        }
    }
    // Resolve again and recompute: a configuration change since approval is a refusal.
    let Some(name) = row
        .args
        .get(keys::ARG_UNIT)
        .and_then(|typed| TargetName::parse(typed).ok())
    else {
        return Err(refuse(shared, held, row, "the unit name is unreadable".to_owned()).await);
    };
    let unit = match deps.resolver.resolve_unit(host, &name).await {
        Ok(unit) => unit,
        Err(TargetError::UnitRefused { refusal, .. }) => {
            let reason = format!("{refusal}: {}", refusal.describe());
            return Err(refuse(shared, held, row, reason).await);
        }
        Err(_) => {
            let reason = "the unit can no longer be resolved".to_owned();
            return Err(refuse(shared, held, row, reason).await);
        }
    };
    let plan = CapabilityPlan::unit_restart(
        host,
        &unit.target.host_key,
        &unit.target.identity,
        &unit.identity_hash,
        &row.args,
        ExecutionId::from_step(step),
    )
    .hash();
    let epoch = commit_dispatch(shared, held, row, plan, &session).await?;
    // The writer's answer is not proof the step is still dispatched: an outside decision may have
    // settled it while this run held the gate.
    if !super::execute::still_dispatched(shared, row, epoch).await {
        return Err(ExecuteError::NotDispatched);
    }
    let kind = row
        .authorization_kind()
        .ok_or(ExecuteError::NotAuthorized)?;
    let expires_at = match row.permit() {
        Some(stored) if kind == AuthorizationKind::Policy => {
            stored.permit().expires_at().min(row.expires_at)
        }
        _ => row.expires_at,
    };
    let deadline = deps
        .clock
        .now()
        .checked_add(RESTART_TIMEOUT)
        .and_then(|t| t.checked_add(shared.settings.dispatch_grace))
        .unwrap_or(now);
    let request = UnitDispatch {
        execution: ExecutionId::from_step(step),
        call: UnitCall::Restart {
            id: unit.id.clone(),
            identity_hash: unit.identity_hash,
        },
        plan_hash: plan,
        authorization_kind: kind,
        expires_at,
    };
    let handoff = match autonomy
        .relay
        .dispatch_restart(host, request, session.session, epoch, deadline)
        .await
    {
        Ok(handoff) => handoff,
        Err(err) => {
            // The hand-off only fails before the frame enters the session's queue, so the restart
            // never reached the agent. The gate is still held.
            tracing::warn!(%step, %err, "the hub never handed the restart to the agent");
            let ending = Ending::new(operation, step, StepOutcome::NotRun).because(HANDOFF_FAILED);
            log(settle(shared, ending).await, row);
            return Ok(());
        }
    };
    drop(gate);
    match end_of(autonomy.relay.restart_result(host, handoff, deadline).await) {
        End::Settle(outcome, reason, output) => {
            let mut ending = Ending::new(operation, step, outcome);
            ending.reason = reason;
            ending.output = output.map(|text| deps.redactor.redact(&text));
            log(settle(shared, ending).await, row);
        }
        End::Unknown => {
            // The restart was handed over and no usable answer came back. It may have happened, so
            // it stays dispatched until the agent's journal settles it; a reconcile asks now.
            tracing::warn!(%step, "the restart's result is unavailable; it stays dispatched");
            if deps.journal.epoch_of(host) == Some(epoch)
                && let Err(err) = queue_reconcile(shared, operation, step).await
            {
                tracing::warn!(%err, %step, "could not queue a reconcile");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn outputs(verification: &str) -> UnitOutputs {
        UnitOutputs(BTreeMap::from([
            (keys::OUT_VERIFICATION.to_owned(), verification.to_owned()),
            (keys::OUT_INVOCATION_BEFORE.to_owned(), "aa".to_owned()),
            (keys::OUT_INVOCATION_AFTER.to_owned(), "bb".to_owned()),
        ]))
    }

    fn rejected(text: &str) -> End {
        end_of(Err(LinkError::Rejected(text.to_owned())))
    }

    fn outcome(end: &End) -> Option<StepOutcome> {
        match end {
            End::Settle(outcome, ..) => Some(*outcome),
            End::Unknown => None,
        }
    }

    #[test]
    fn outcome_mapping_table() {
        // A verified success, and an unverified one that says so.
        let verified = end_of(Ok(outputs("verified")));
        assert!(matches!(
            &verified,
            End::Settle(StepOutcome::Succeeded, None, Some(_))
        ));
        let unverified = end_of(Ok(outputs("unverified")));
        let End::Settle(StepOutcome::Succeeded, Some(reason), _) = unverified else {
            panic!("an unverified success keeps a reason");
        };
        assert_eq!(reason, "restarted; the new process could not be checked");

        // The agent's own refusals never count against a rule.
        for token in REFUSED_FRAMES {
            assert_eq!(
                outcome(&rejected(&format!("{token}: some detail"))),
                Some(StepOutcome::Refused),
                "{token}"
            );
        }
        // The helper's tokens that prove nothing restarted are `Refused`.
        for token in [
            "target_not_found",
            "target_protected",
            "target_not_allowlisted",
            "target_stale",
            "target_changed: loaded",
            "read_failed",
            "platform_untested",
            "platform_blocked",
            "restart_budget_exhausted",
            "target_unsupported: masked",
        ] {
            assert_eq!(
                outcome(&rejected(token)),
                Some(StepOutcome::Refused),
                "{token}"
            );
        }
        // A restart that began and did not end well is `Failed`, which counts for the breaker.
        for token in [
            "restart_failed",
            "start_limit_hit",
            "no_restart_observed",
            "wrong_start: environment",
        ] {
            assert_eq!(
                outcome(&rejected(token)),
                Some(StepOutcome::Failed),
                "{token}"
            );
        }
        // An unrecognised message is `Failed`, cut to 500 characters.
        let End::Settle(StepOutcome::Failed, Some(reason), _) = rejected(&"x".repeat(900)) else {
            panic!("an unrecognised answer fails");
        };
        assert_eq!(reason.chars().count(), 500);
        // No usable answer proves nothing either way.
        assert_eq!(end_of(Err(LinkError::Timeout)), End::Unknown);
        assert_eq!(
            end_of(Err(LinkError::Indeterminate("lost".to_owned()))),
            End::Unknown
        );
    }
}
