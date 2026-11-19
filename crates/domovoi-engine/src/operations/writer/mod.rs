//! The one writer task of the operation aggregate. Every change to an operation arrives as a
//! command on a bounded queue and is applied one at a time: load a fresh snapshot, decide,
//! commit (state, journal, audit and effects together). Because nothing else writes
//! operation state, a decision is never made over a view somebody else has since changed.
//!
//! [`process`] is the only place [`OperationCommand::Dispatch`] is matched.

mod commands;
mod commit;
mod create;
mod dispatch;
mod execution;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use domovoi_core::OperationId;
use tokio::sync::{Notify, mpsc};

pub(crate) use commands::{Held, OperationCommand, Reply};

use commit::{Applied, Change, Edit, apply, finish_held, queue_reconcile};
use execution::system;

use super::decide::{EffectSpec, OperationEvent};
use super::deps::OperationDeps;
use super::error::OperationError;
use super::settings::OperationSettings;

/// Commands waiting to be applied queue up at most this deep.
const WRITE_QUEUE: usize = 64;

/// What the writer and the handlers share: the ports, the settings and the way to reach the
/// writer.
pub(crate) struct OpShared {
    pub(crate) deps: OperationDeps,
    pub(crate) settings: OperationSettings,
    /// The queue into the writer task.
    pub(crate) writer: mpsc::Sender<OperationCommand>,
    /// Wakes the effect runner as soon as a commit creates an effect that is due.
    pub(crate) nudge: Arc<Notify>,
}

/// Opens the queue into the writer; the receiver goes to [`run`].
pub(crate) fn queue() -> (
    mpsc::Sender<OperationCommand>,
    mpsc::Receiver<OperationCommand>,
) {
    mpsc::channel(WRITE_QUEUE)
}

/// Applies commands one at a time until shutdown closes the queue or every handle is
/// dropped, then applies what is still queued.
pub(crate) async fn run(shared: Arc<OpShared>, mut rx: mpsc::Receiver<OperationCommand>) {
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let command = tokio::select! {
            () = shutdown.cancelled() => break,
            command = rx.recv() => match command {
                Some(command) => command,
                None => break,
            },
        };
        process(&shared, command).await;
    }
    while let Ok(command) = rx.try_recv() {
        process(&shared, command).await;
    }
}

/// Applies `change`, answers its caller, and admits queued operations when a slot freed up.
async fn answer(shared: &OpShared, change: Change, reply: Reply<()>) {
    let result = apply(shared, change).await;
    let freed = matches!(
        &result,
        Ok(Applied {
            became_terminal: true
        })
    );
    if let Err(err) = &result {
        tracing::debug!(%err, "an operation command was refused");
    }
    let _ = reply.send(result.map(|_| ()));
    if freed {
        admit_queued(shared).await;
    }
}

/// A change for `operation` made by the service itself.
fn by_system(operation: OperationId, event: OperationEvent) -> Change {
    Change::new(operation, system(), event)
}

/// A change for `operation` with a held effect.
fn held_change(held: Held, operation: OperationId, event: OperationEvent) -> Change {
    let mut change = by_system(operation, event);
    change.held = Some(held);
    change
}

/// Applies one command and answers its caller, whatever the outcome was.
pub(crate) async fn process(shared: &OpShared, command: OperationCommand) {
    match command {
        OperationCommand::Create { seed, reply } => {
            let result = create::create(shared, *seed).await;
            if let Err(err) = &result {
                tracing::debug!(%err, "an operation could not be created");
            }
            let _ = reply.send(result);
            admit_queued(shared).await;
        }
        OperationCommand::Propose {
            operation,
            steps,
            edges,
            actor,
            reply,
        } => {
            let event = OperationEvent::Propose { steps, edges };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::ProposeExecution {
            operation,
            proposal,
            actor,
            reply,
        } => {
            let event = OperationEvent::ProposeExecution(*proposal);
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::Authorize {
            operation,
            step,
            authorization,
            binding,
            provenance,
            reply,
        } => {
            let change = execution::authorize(operation, step, authorization, binding, provenance);
            answer(shared, change, reply).await;
        }
        OperationCommand::Deny {
            operation,
            step,
            approval,
            reply,
        } => answer(shared, execution::deny(operation, step, &approval), reply).await,
        OperationCommand::Answer {
            operation,
            question,
            note,
            actor,
            reply,
        } => {
            let event = OperationEvent::Answer { question, note };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::StepStarted {
            operation,
            step,
            actor,
            reply,
        } => {
            let event = OperationEvent::StepStarted { step };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::StepFinished {
            operation,
            step,
            outcome,
            summary,
            reason,
            output,
            usage,
            actor,
            reply,
        } => {
            let redact = |text: String| shared.deps.redactor.redact(&text);
            let event = OperationEvent::StepFinished {
                step,
                outcome,
                summary: summary.map(redact),
                reason: reason.map(redact),
                output: output.map(redact),
                usage,
            };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::Cancel {
            operation,
            actor,
            reason,
            reply,
        } => {
            let reason = shared.deps.redactor.redact(&reason);
            let event = OperationEvent::Cancel { reason };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::Pause {
            operation,
            actor,
            reply,
        } => {
            let event = OperationEvent::Pause {
                reason: domovoi_core::PauseReason::GlobalPause,
            };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::Resume {
            operation,
            actor,
            extend,
            reply,
        } => {
            let event = OperationEvent::Resume { extend };
            answer(shared, Change::new(operation, actor, event), reply).await;
        }
        OperationCommand::Finish {
            operation,
            outcome,
            summary,
            reply,
        } => {
            let summary = shared.deps.redactor.redact(&summary);
            let event = OperationEvent::Finish { outcome, summary };
            answer(shared, by_system(operation, event), reply).await;
        }
        OperationCommand::ReconcileHost { host, reply } => {
            reconcile_host(shared, host).await;
            let _ = reply.send(());
        }
        OperationCommand::StartStep {
            held,
            operation,
            step,
            reply,
        } => {
            let event = OperationEvent::StepStarted { step };
            let mut change = by_system(operation, event);
            change.fence = Some(held);
            answer(shared, change, reply).await;
        }
        OperationCommand::FinishStep {
            held,
            operation,
            step,
            outcome,
            summary,
            reason,
            output,
            reply,
        } => {
            let redact = |text: String| shared.deps.redactor.redact(&text);
            let event = OperationEvent::StepFinished {
                step,
                outcome,
                summary: summary.map(redact),
                reason: reason.map(redact),
                output: output.map(redact),
                usage: None,
            };
            answer(shared, held_change(held, operation, event), reply).await;
        }
        OperationCommand::Defer {
            held,
            operation,
            step,
            successor,
            waiting,
            reply,
        } => {
            let result = defer(shared, held, operation, step, *successor, waiting).await;
            let _ = reply.send(result);
        }
        OperationCommand::Dispatch {
            held,
            operation,
            step,
            plan,
            session,
            epoch,
            reply,
        } => {
            let request = dispatch::Request {
                held,
                operation,
                step,
                plan,
                session,
                epoch,
            };
            let result = dispatch::dispatch(shared, request).await;
            if let Err(err) = &result {
                tracing::debug!(%err, "a dispatch was refused");
            }
            let _ = reply.send(result);
        }
        OperationCommand::Settle {
            held,
            operation,
            step,
            outcome,
            reason,
            output,
            verify_after,
            reply,
        } => {
            let settled = execution::Settled {
                held,
                operation,
                step,
                outcome,
                reason,
                output,
                verify_after,
            };
            answer(shared, execution::settle(shared, settled), reply).await;
        }
        OperationCommand::Verified {
            held,
            operation,
            step,
            passed,
            summary,
            reply,
        } => {
            let summary = shared.deps.redactor.redact(&summary);
            let event = OperationEvent::Verified {
                step,
                passed,
                summary,
            };
            answer(shared, held_change(held, operation, event), reply).await;
        }
        OperationCommand::Expire {
            held,
            operation,
            step,
            reply,
        } => {
            let event = OperationEvent::ExecutionExpired { step };
            answer(shared, held_change(held, operation, event), reply).await;
        }
        OperationCommand::Tick {
            held,
            operation,
            reply,
        } => {
            answer(
                shared,
                held_change(held, operation, OperationEvent::Tick),
                reply,
            )
            .await;
        }
        OperationCommand::FinishEffect { held, reply } => {
            let result = finish_held_effect(shared, held).await;
            let _ = reply.send(result);
        }
    }
}

/// Finishes the held effect and creates its successor in one commit; with `waiting` the
/// operation also records that it now waits for the step's agent.
async fn defer(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: Option<domovoi_core::StepId>,
    successor: EffectSpec,
    waiting: bool,
) -> Result<(), OperationError> {
    let (true, Some(step)) = (waiting, step) else {
        return finish_held(shared, held, operation, Some(successor)).await;
    };
    let mut change = held_change(held, operation, OperationEvent::WaitingForAgent { step });
    change.edit = Edit::Waiting(step);
    change
        .effects
        .push(commit::without_held_key(shared, held, successor).await?);
    apply(shared, change).await.map(|_| ())
}

/// Completes a held effect that has nothing left to do. An effect is addressed to an
/// operation, so a stale claim changes nothing.
async fn finish_held_effect(shared: &OpShared, held: Held) -> Result<(), OperationError> {
    let record = shared
        .deps
        .effect_read
        .effect(held.effect)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?;
    let Some(record) = record else {
        return Err(OperationError::StaleEffect);
    };
    let operation = record
        .aggregate
        .id
        .parse()
        .map_err(|_| OperationError::Store("an effect names no operation".to_owned()))?;
    finish_held(shared, held, operation, None).await
}

/// Queues a reconcile for each execution dispatched to `host`.
async fn reconcile_host(shared: &OpShared, host: domovoi_core::HostId) {
    let rows = match shared.deps.operations.dispatched_on(host).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(%err, %host, "could not list the executions dispatched to a host");
            return;
        }
    };
    for row in rows {
        if let Err(err) = queue_reconcile(shared, row.operation, row.step).await {
            tracing::warn!(%err, step = %row.step, "could not queue a reconcile");
        }
    }
}

/// The statuses that hold one of the `max_running` slots. A paused operation keeps its slot.
const ACTIVE: [&str; 6] = [
    "running",
    "waiting_approval",
    "waiting_user",
    "waiting_agent",
    "waiting_timer",
    "paused",
];

/// Starts queued operations, oldest first, while a slot is free. Runs inside the writer (or
/// before it starts), never through its queue.
pub(crate) async fn admit_queued(shared: &OpShared) {
    let operations = &shared.deps.operations;
    let held = match operations
        .with_status(&ACTIVE, shared.settings.max_running)
        .await
    {
        Ok(active) => active.len(),
        Err(err) => {
            tracing::warn!(%err, "could not count the running operations");
            return;
        }
    };
    let free = usize::from(shared.settings.max_running).saturating_sub(held);
    let Ok(free) = u16::try_from(free) else {
        return;
    };
    if free == 0 {
        return;
    }
    let queued = match operations.with_status(&["queued"], free).await {
        Ok(queued) => queued,
        Err(err) => {
            tracing::warn!(%err, "could not list the queued operations");
            return;
        }
    };
    for operation in queued {
        let change = by_system(operation.id, OperationEvent::Admit);
        if let Err(err) = apply(shared, change).await {
            tracing::debug!(%err, operation = %operation.id, "a queued operation was not admitted");
        }
    }
}
