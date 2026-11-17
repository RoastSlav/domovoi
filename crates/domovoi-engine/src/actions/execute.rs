//! The one path from an approved action to a running skill.
//!
//! [`execute`] is the only function that decides to dispatch and the only caller of
//! [`AgentLink::start_skill`](domovoi_core::ports::AgentLink::start_skill). It takes an
//! [`AuthorizedExecution`] by value, so a run can only be dispatched from a row that still
//! carries the authorization a human gave it: the type has no public constructor, is never
//! serialised and never reaches the store. `xtask arch-check` (EXEC-2) fails when another
//! module starts a skill.
//!
//! Everything a run writes still goes through the one writer task (ADR 0019): the dispatch,
//! the loop guard's rejection, the answer for the run and the re-check's delay. This module
//! reads the incident and the agent session it is handed, and never stores anything.

use domovoi_core::ports::AgentSnapshot;
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, AuthorizationKind, ExecutionId, Incident, PlanFamily,
    PlanHash, SkillSummary, ValidatedArgs,
};
use jiff::SignedDuration;
use tokio::sync::oneshot;

use super::Shared;
use super::audit::RunRecord;
use super::executor::ask;
use super::preflight;
use super::recheck::verify_after;
use super::record::run_record;
use super::writer::WriteCommand;

/// An action a human authorized, as the one thing [`execute`] may dispatch.
///
/// It exists for the length of one run: [`AuthorizedExecution::of`] is its only constructor,
/// it is never stored, logged in full or serialised, and the action it carries can only be
/// read through [`AuthorizedExecution::action`].
#[derive(Debug)]
pub struct AuthorizedExecution {
    action: ActionRequest,
    execution_id: ExecutionId,
}

impl AuthorizedExecution {
    /// The one constructor: it refuses a row with no stored human approval.
    ///
    /// The status is not checked here; the caller only builds this for a row it has already
    /// read as `Approved`. A policy authorization is refused with
    /// [`ExecuteError::NotYetSupported`], the same answer the writer gives, because no
    /// permit is issued in this milestone.
    pub(crate) fn of(action: ActionRequest) -> Result<Self, ExecuteError> {
        match action.authorization_kind() {
            Some(AuthorizationKind::Human) => {}
            Some(AuthorizationKind::Policy) => return Err(ExecuteError::NotYetSupported),
            None => return Err(ExecuteError::NotAuthorized),
        }
        if action.approved_by().is_none() {
            return Err(ExecuteError::NotAuthorized);
        }
        let execution_id = ExecutionId::from_action(action.id());
        Ok(Self {
            action,
            execution_id,
        })
    }

    /// The action as it will be dispatched.
    #[must_use]
    pub fn action(&self) -> &ActionRequest {
        &self.action
    }

    /// The idempotency key of this run; at M1 it equals the action id.
    #[must_use]
    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }
}

/// Why a run was not dispatched.
///
/// Every variant is a refusal that writes nothing of its own: the row keeps the status the
/// writer gave it, and the caller only logs. A rejection the writer makes (a changed plan, a
/// closed window) is already stored by the writer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecuteError {
    /// The action carries no stored human approval.
    #[error("the action has no valid human authorization")]
    NotAuthorized,
    /// The action was authorized by a policy permit, which this milestone refuses.
    #[error("policy authorizations are not supported yet")]
    NotYetSupported,
    /// The approval window closed before the run reached dispatch.
    #[error("the approval expired before the action was dispatched")]
    Expired,
    /// The plan the agent has now is not the approved one, so the writer rejected it.
    #[error("the plan changed since it was shown; the action was not dispatched")]
    PlanChanged,
    /// The writer is closing or answered without a dispatch, so nothing was sent.
    #[error("the run was not dispatched")]
    NotDispatched,
    /// A check before dispatch refused the run with this reason.
    #[error("{0}")]
    Refused(String),
}

/// Sends one authorized action to its host's agent, after the checks the executor owns.
///
/// The caller has already read the incident, the agent session and the skill catalogue, and
/// has validated the stored arguments against the current skill: this function takes those
/// reads so the plan, the dispatch guard and the request all describe one session. It
/// reserves the loop guard's slot, recomputes the approval-family hash, queues the single
/// [`WriteCommand::Dispatch`], sends the request only when the writer answers `Dispatched`,
/// and settles the run's answer through the writer.
///
/// Two hashes are computed and kept apart: the approval-family hash the dispatch guard
/// compares, and the always-v1 `runskill_plan_hash` carried on the wire, which the agent's
/// own local check recomputes.
pub(crate) async fn execute(
    shared: &Shared,
    authorized: AuthorizedExecution,
    skill: SkillSummary,
    args: ValidatedArgs,
    session: AgentSnapshot,
    incident: Incident,
) -> Result<(), ExecuteError> {
    let action = authorized.action();
    let id = action.id();
    let deps = &shared.deps;
    let now = deps.clock.now();
    // The executor's preflights resolve an expired window through the writer before this
    // point; this is the precondition's own guard for a direct call, and it writes nothing.
    if now >= action.expires_at() {
        return Err(ExecuteError::Expired);
    }
    // The slot is claimed here, before the plan is even recomputed, so two catch-up runs
    // of the same skill cannot both read the same count and both pass (ADR 0019's writer
    // answers the dispatch, but the guard is this function's own bookkeeping).
    if let Some(reason) = preflight::reserve_dispatch(shared, action, now) {
        let _ = preflight::ask_reject(shared, id, reason.clone(), true).await;
        return Err(ExecuteError::Refused(reason));
    }
    // The wire hash is always v1: the agent recomputes it from its manifest before running.
    let runskill_plan_hash =
        PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &args);
    // The approval-family hash is what the dispatch guard compares; for v2 it is recomputed
    // from the host's current enrolled key and the session this request will be sent to.
    let (approval_plan_hash, plan_target) = match action.approval_plan().family() {
        PlanFamily::V2 => {
            let Some((hash, target)) =
                super::v2_plan::v2_plan(shared, action, &skill, &args, &incident, &session).await
            else {
                return Err(ExecuteError::Refused(
                    "the plan can no longer be verified".to_owned(),
                ));
            };
            (hash, Some(target))
        }
        PlanFamily::V1 => (
            PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &args),
            None,
        ),
    };
    // The gate closes the window between the write that says `Dispatched` and the frame
    // reaching the agent's queue: reconciliation takes it before it lists what is pending,
    // so a run that has not been handed over yet can never be in that list.
    let gate = shared.dispatch_gate(action.host()).await;
    let Some(dispatched) = ask_dispatch(
        shared,
        id,
        approval_plan_hash,
        plan_target,
        session.session,
        session.journal_epoch,
    )
    .await
    else {
        return Err(ExecuteError::NotDispatched);
    };
    let (epoch, skill_name, args) = match dispatched.status() {
        ActionStatus::Dispatched {
            agent_epoch: Some(epoch),
        } => (*epoch, skill.name.clone(), args),
        ActionStatus::Dispatched { agent_epoch: None } => {
            // The hub saw no journal when it committed the dispatch, so there is no session
            // the run may safely be handed to. It stays dispatched and reconciliation
            // settles it as unknown; sending it could never be accounted for.
            tracing::warn!(
                action = %id,
                execution = %authorized.execution_id(),
                "the dispatch recorded no journal epoch; the run is not sent",
            );
            return Err(ExecuteError::NotDispatched);
        }
        ActionStatus::Rejected { reason } => {
            tracing::debug!(
                action = %id,
                execution = %authorized.execution_id(),
                %reason,
                "the plan the agent has now is not the approved one",
            );
            return Err(ExecuteError::PlanChanged);
        }
        ActionStatus::NotRun { .. } => return Err(ExecuteError::Expired),
        _ => {
            tracing::debug!(
                action = %id,
                execution = %authorized.execution_id(),
                "the writer did not dispatch the run",
            );
            return Err(ExecuteError::NotDispatched);
        }
    };
    // The writer's answer is not proof the row is still dispatched: an outside decision may
    // have settled it while this run held the gate. Re-read before anything is sent.
    let current = match deps.actions.get(id).await {
        Ok(Some(current)) => current,
        Ok(None) => {
            tracing::debug!(action = %id, "the action vanished before it could be sent");
            return Err(ExecuteError::NotDispatched);
        }
        Err(err) => {
            tracing::warn!(%err, action = %id, "could not re-read the action before sending it");
            return Err(ExecuteError::NotDispatched);
        }
    };
    if !matches!(
        current.status(),
        ActionStatus::Dispatched {
            agent_epoch: Some(seen),
        } if *seen == epoch
    ) {
        tracing::debug!(
            action = %id,
            "the action is no longer dispatched under its journal epoch; it is not sent",
        );
        return Err(ExecuteError::NotDispatched);
    }
    let deadline = deps
        .clock
        .now()
        .checked_add(skill.timeout)
        .and_then(|t| t.checked_add(shared.settings.dispatch_grace))
        .unwrap_or(now);
    let dispatch = domovoi_core::ports::SkillDispatch {
        action: id,
        skill: skill_name,
        args,
        // The agent's local check recomputes v1, so a v2 value here would refuse every new
        // action; `approval_plan_hash` is the value the dispatch guard uses, not this one.
        plan_hash: runskill_plan_hash,
    };
    let handoff = match deps
        .handoff
        .hand_off(action.host(), dispatch, session.session, epoch, deadline)
        .await
    {
        Ok(handoff) => handoff,
        Err(err) => {
            // `start_skill` only fails before the frame enters the session's outgoing queue,
            // so this is proof the request never reached the agent. Settle it as a run that
            // never happened, through the writer. The dispatch gate is still held, so a
            // reconciliation pass cannot read the row as one that was sent and lost.
            tracing::warn!(
                action = %id,
                %err,
                "the hub never handed the run to the agent; it did not run",
            );
            super::reconcile::handoff_failed(shared, id).await;
            return Err(ExecuteError::NotDispatched);
        }
    };
    drop(gate);
    match deps
        .handoff
        .await_handoff_result(action.host(), handoff, deadline)
        .await
    {
        Ok(output) => {
            let record = run_record(shared, &output);
            let outcome = match output.exit_code {
                Some(0) => Ok(()),
                Some(code) => Err(format!("the skill exited with code {code}")),
                None => Err("the skill was killed".to_owned()),
            };
            let ok = outcome.is_ok();
            // The re-check is a durable effect created with the success transition, so the
            // writer needs the delay now; the runner fires it when the delay has passed.
            let delay = if ok {
                verify_after(shared, action, &skill).await
            } else {
                None
            };
            let _ = ask_complete(shared, id, outcome, Some(record), delay).await;
            Ok(())
        }
        Err(domovoi_core::ports::LinkError::Rejected(reason)) => {
            // A terminal answer from the agent: it refused the run or reported a failure.
            let _ = ask_complete(shared, id, Err(reason), None, None).await;
            Ok(())
        }
        Err(err) => {
            // The request was handed over and no usable answer came back. The run may have
            // happened, so it is neither a success nor a failure: it stays dispatched until
            // the agent's journal can settle it. Ask for that pass now, while the session
            // that could still remember the run is live; a replacement's own pass covers a
            // changed journal.
            tracing::warn!(
                action = %id,
                %err,
                "the run's result is unavailable; it stays dispatched for reconciliation",
            );
            if deps.journal.epoch_of(action.host()) == Some(epoch) {
                shared.schedule_reconciliation(action.host());
            }
            Ok(())
        }
    }
}

/// Queues `Dispatch` and answers with the action as the writer left it: `Dispatched`
/// when the plan still matches, `Rejected` when it no longer does (INV-4).
async fn ask_dispatch(
    shared: &Shared,
    id: ActionId,
    plan: PlanHash,
    target: Option<domovoi_core::PlanTarget>,
    session: domovoi_core::AgentSessionId,
    epoch: domovoi_core::JournalEpoch,
) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "a dispatch",
        WriteCommand::Dispatch {
            id,
            plan,
            target,
            session,
            epoch,
            reply,
        },
        answer,
    )
    .await
}

/// Queues the agent's answer for a run, with the delay the verification owes if it succeeded.
async fn ask_complete(
    shared: &Shared,
    id: ActionId,
    outcome: Result<(), String>,
    record: Option<RunRecord>,
    verify_after: Option<SignedDuration>,
) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "the answer for a run",
        WriteCommand::Complete {
            id,
            result: outcome,
            record,
            verify_after,
            reply,
        },
        answer,
    )
    .await
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
