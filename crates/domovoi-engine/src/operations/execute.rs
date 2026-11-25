//! The one path from an approved Execution step to a running skill.
//!
//! [`execute_step`] is the only function in the operation service that decides to dispatch, and
//! [`AuthorizedStep`] is what it takes: a value that can only be built from a row that still
//! carries the approval a person gave it. The type has no public constructor, is never stored
//! or serialised, and is built once, by the dispatch handler. `xtask arch-check` (EXEC-2) fails
//! when another module hands a skill over, builds the dispatch command or builds an
//! [`AuthorizedStep`].
//!
//! This is the operation service's own copy of the dispatch boundary in `actions::execute`;
//! the two stay apart until the legacy executor is removed. Everything a run writes still goes
//! through the one writer: the dispatch, the loop guard's refusal, the answer for the run and
//! the re-check's delay. This module reads what it is handed and stores nothing itself.

use domovoi_core::capability::TargetQuery;
use domovoi_core::ports::{AgentSnapshot, LinkError, SkillDispatch, SkillOutput};
use domovoi_core::{
    ExecutionError, ExecutionId, ExecutionRecord, PlanHash, PlanTarget, Risk, SkillSummary,
    StepOutcome, ValidatedArgs,
};
use jiff::SignedDuration;
use tokio::sync::oneshot;

use super::audit::LOOP_GUARD_REASON;
use super::effects::{Ending, settle};
use super::proposal::plan_hash;
use super::settings::OperationSettings;
use super::writer::{DispatchAnswer, Held, OpShared, OperationCommand, queue_reconcile};

/// The reason the hub gives for a run it proved it never handed to the agent.
const HANDOFF_FAILED: &str = "hub_not_handed_off";

/// An Execution step a person authorized, as the one thing [`execute_step`] may dispatch.
///
/// It exists for the length of one run: [`AuthorizedStep::of`] is its only constructor and
/// the record it carries can only be read through [`AuthorizedStep::execution`].
#[derive(Debug)]
pub struct AuthorizedStep {
    execution: ExecutionRecord,
}

impl AuthorizedStep {
    /// The one constructor: it refuses a row with no stored human approval. A stored approval
    /// is always a person's, because [`ExecutionRecord::authorize`] answers a policy
    /// authorization with [`ExecuteError::NotYetSupported`] (see the `From` impl) before
    /// anything is stored.
    pub(super) fn of(execution: ExecutionRecord) -> Result<Self, ExecuteError> {
        if execution.approval().is_none() {
            return Err(ExecuteError::NotAuthorized);
        }
        Ok(Self { execution })
    }

    /// The execution as it will be dispatched.
    #[must_use]
    pub fn execution(&self) -> &ExecutionRecord {
        &self.execution
    }
}

/// Why a run was not dispatched. A refusal the writer made (a changed plan, a closed window)
/// is already stored by the writer; the others write nothing of their own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecuteError {
    /// The row carries no stored human approval.
    #[error("the execution has no valid human authorization")]
    NotAuthorized,
    /// The row was authorized by a policy permit, which is not supported yet.
    #[error("policy authorizations are not supported yet")]
    NotYetSupported,
    /// The approval window closed before the run reached dispatch.
    #[error("the approval expired before the action was dispatched")]
    Expired,
    /// The plan the agent has now is not the approved one, so the writer refused it.
    #[error("the plan changed since it was shown; the action was not dispatched")]
    PlanChanged,
    /// The writer answered without a dispatch, or the step is no longer dispatched, so
    /// nothing was sent.
    #[error("the run was not dispatched")]
    NotDispatched,
    /// A check before dispatch refused the run with this reason.
    #[error("{0}")]
    Refused(String),
}

impl From<ExecutionError> for ExecuteError {
    fn from(err: ExecutionError) -> Self {
        match err {
            ExecutionError::AlreadyAuthorized
            | ExecutionError::NeedsTypedConfirmation
            | ExecutionError::PermitNotAllowed => Self::NotAuthorized,
            ExecutionError::Expired => Self::Expired,
            ExecutionError::PlanChanged => Self::PlanChanged,
            ExecutionError::NotYetSupported => Self::NotYetSupported,
        }
    }
}

/// How long to wait before re-checking after a success, or `None` when the run is not
/// verified. A skill that says how to verify itself wins; otherwise a write skill that applies
/// to the pinned check waits `settings.verify_default_delay`.
pub(super) fn verify_delay(
    settings: &OperationSettings,
    skill: Option<&SkillSummary>,
    execution: &ExecutionRecord,
) -> Option<SignedDuration> {
    let skill = skill?;
    match &skill.verify {
        Some(spec) if spec.recheck => Some(spec.delay),
        Some(_) => None,
        None => (skill.risk == Risk::Write && skill.applies_to.contains(&execution.check))
            .then_some(settings.verify_default_delay),
    }
}

/// Sends one authorized execution to its host's agent, after the checks this boundary owns.
///
/// The handler has already read the session and the skill catalogue and validated the stored
/// arguments against the current skill; this function takes those reads so the plan, the
/// dispatch guard and the request all describe one session. It takes the host's dispatch gate,
/// applies the loop guard, recomputes the plan hash, queues the single dispatch command, sends
/// the request only when the writer answers that the run is dispatched, and settles the run's
/// answer through the writer. `held` is the dispatch effect the writer completes in the same
/// commit as the dispatch.
///
/// Two hashes are computed and kept apart: the v2 hash the writer compares, and the always-v1
/// hash carried on the wire, which the agent recomputes from its own manifest.
pub(super) async fn execute_step(
    shared: &OpShared,
    held: Held,
    authorized: AuthorizedStep,
    skill: SkillSummary,
    args: ValidatedArgs,
    session: AgentSnapshot,
) -> Result<(), ExecuteError> {
    let row = authorized.execution();
    let (operation, step, host) = (row.operation, row.step, row.host);
    let deps = &shared.deps;
    let now = deps.clock.now();
    if now >= row.expires_at {
        return Err(ExecuteError::Expired);
    }
    // The gate closes the window between the commit that says dispatched and the frame
    // reaching the agent's queue: reconciliation takes it before it reads the execution, so a
    // run that has not been handed over yet is never read as one the agent lost. It is taken
    // before the loop guard counts, so two dispatches cannot both read the same count.
    let gate = shared.dispatch_gate(host).await;
    let since = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let recent = match deps
        .operations
        .dispatched_since(host, &row.contract, since)
        .await
    {
        Ok(count) => count,
        Err(err) => {
            tracing::warn!(%err, %step, "could not count the recent dispatches; not dispatching");
            return Err(ExecuteError::NotDispatched);
        }
    };
    let limit = shared.settings.loop_guard_per_hour;
    if recent >= limit {
        let reason = format!(
            "{LOOP_GUARD_REASON} {} already ran {limit} times on this host in the last hour",
            skill.name
        );
        return Err(refuse(shared, held, row, reason).await);
    }
    let Some(approval_hash) = recompute(shared, row, &skill, &args).await else {
        let reason = "the plan can no longer be verified".to_owned();
        return Err(refuse(shared, held, row, reason).await);
    };
    let wire_hash = PlanHash::compute(&skill.name, skill.manifest_hash, host, &args);
    let epoch = session.journal_epoch;
    let (reply, answer) = oneshot::channel();
    let command = OperationCommand::Dispatch {
        held,
        operation,
        step,
        plan: approval_hash,
        session: session.session,
        epoch,
        reply,
    };
    if shared.writer.send(command).await.is_err() {
        return Err(ExecuteError::NotDispatched);
    }
    match answer.await {
        Ok(Ok(DispatchAnswer::Dispatched(_))) => {}
        Ok(Ok(DispatchAnswer::Settled(StepOutcome::Refused))) => {
            return Err(ExecuteError::PlanChanged);
        }
        Ok(Ok(DispatchAnswer::Settled(StepOutcome::NotRun))) => return Err(ExecuteError::Expired),
        _ => return Err(ExecuteError::NotDispatched),
    }
    // The writer's answer is not proof the step is still dispatched: an outside decision may
    // have settled it while this run held the gate. Read it again before anything is sent.
    if !still_dispatched(shared, row, epoch).await {
        tracing::debug!(%step, "the step is no longer dispatched under its journal epoch; not sent");
        return Err(ExecuteError::NotDispatched);
    }
    let deadline = deps
        .clock
        .now()
        .checked_add(skill.timeout)
        .and_then(|t| t.checked_add(shared.settings.dispatch_grace))
        .unwrap_or(now);
    let dispatch = SkillDispatch {
        action: ExecutionId::from_step(step).as_action(),
        skill: skill.name.clone(),
        args,
        // The agent's local check recomputes v1, so a v2 value here would refuse every run.
        plan_hash: wire_hash,
    };
    let handoff = match deps
        .handoff
        .hand_off(host, dispatch, session.session, epoch, deadline)
        .await
    {
        Ok(handoff) => handoff,
        Err(err) => {
            // The hand-off only fails before the frame enters the session's outgoing queue,
            // so the run never reached the agent. The gate is still held, so a
            // reconciliation cannot read the step as one that was sent and lost.
            tracing::warn!(%step, %err, "the hub never handed the run to the agent; it did not run");
            let ending = Ending::new(operation, step, StepOutcome::NotRun).because(HANDOFF_FAILED);
            log_settle(settle(shared, ending).await, step);
            return Ok(());
        }
    };
    drop(gate);
    match deps
        .handoff
        .await_handoff_result(host, handoff, deadline)
        .await
    {
        Ok(output) => finished(shared, row, &skill, &output).await,
        Err(LinkError::Rejected(reason)) => {
            let ending = Ending::new(operation, step, StepOutcome::Failed).because(reason);
            log_settle(settle(shared, ending).await, step);
        }
        Err(err) => {
            // The request was handed over and no usable answer came back. The run may have
            // happened, so it is neither a success nor a failure: it stays dispatched until
            // the agent's journal can settle it, which a reconcile asks about now.
            tracing::warn!(%step, %err, "the run's result is unavailable; it stays dispatched");
            if deps.journal.epoch_of(host) == Some(epoch)
                && let Err(err) = queue_reconcile(shared, operation, step).await
            {
                tracing::warn!(%err, %step, "could not queue a reconcile");
            }
        }
    }
    Ok(())
}

/// Settles the step from the agent's terminal answer.
async fn finished(
    shared: &OpShared,
    row: &ExecutionRecord,
    skill: &SkillSummary,
    output: &SkillOutput,
) {
    let text = if output.stderr.trim().is_empty() {
        output.stdout.clone()
    } else {
        format!("{}\n--- stderr ---\n{}", output.stdout, output.stderr)
    };
    let text = shared.deps.redactor.redact(&text);
    let mut ending = match output.exit_code {
        Some(0) => Ending::new(row.operation, row.step, StepOutcome::Succeeded),
        Some(code) => Ending::new(row.operation, row.step, StepOutcome::Failed)
            .because(format!("exit code {code}")),
        None => Ending::new(row.operation, row.step, StepOutcome::Failed)
            .because("the skill was killed"),
    };
    if ending.outcome == StepOutcome::Succeeded {
        ending.verify_after = verify_delay(&shared.settings, Some(skill), row);
    }
    ending.output = Some(text);
    log_settle(settle(shared, ending).await, row.step);
}

/// Logs a settlement that was refused: another path may have settled the step first.
fn log_settle(result: Result<(), super::OperationError>, step: domovoi_core::StepId) {
    if let Err(err) = result {
        tracing::debug!(%err, %step, "the result of a run was not recorded");
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
    log_settle(settle(shared, ending).await, row.step);
    ExecuteError::Refused(reason)
}

/// The v2 plan hash recomputed from what the hub has now: the host's current enrolled key,
/// the current manifest and entry digest, the arguments and the step's own identity.
async fn recompute(
    shared: &OpShared,
    row: &ExecutionRecord,
    skill: &SkillSummary,
    args: &ValidatedArgs,
) -> Option<PlanHash> {
    let resolved = shared
        .deps
        .resolver
        .resolve(TargetQuery::Host { host: row.host })
        .await
        .ok()?;
    let target = PlanTarget::new(
        row.host,
        resolved.host_key,
        row.check.clone(),
        row.target.clone(),
    );
    plan_hash(skill, row.contract_version, &target, args, row.step).ok()
}

/// Whether the step is still running under the epoch the writer recorded for the dispatch.
async fn still_dispatched(
    shared: &OpShared,
    row: &ExecutionRecord,
    epoch: domovoi_core::JournalEpoch,
) -> bool {
    let Ok(Some(snapshot)) = shared.deps.operations.load(row.operation).await else {
        return false;
    };
    let running = snapshot
        .step(row.step)
        .is_some_and(|record| record.progress.outcome == StepOutcome::Running);
    let recorded = snapshot
        .execution_of(row.step)
        .and_then(|stored| stored.value.dispatched)
        .map(|(recorded, _)| recorded);
    running && recorded == Some(epoch)
}
