//! Runs approved actions: loop guard, re-validation (INV-4), dispatch, verification.
//!
//! Each `ActionApproved` event starts one run, and actions the store still says are
//! approved are started when the service starts, so an approval made just before a
//! restart still runs (ADR 0014). A run reloads the action and does nothing unless it
//! is still `Approved`, so a repeated event never runs a skill twice. Every change to
//! the action itself is queued for the one writer task (ADR 0019), which answers with
//! the action as it left it, so this file never writes action state.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{LinkError, SkillDispatch, SkillOutput};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, PlanHash};
use jiff::SignedDuration;
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, broadcast, oneshot};
use tokio::task::JoinSet;

use super::Shared;
use super::audit::RunRecord;
use super::host_wait::{self, HostWait};
use super::preflight;
use super::recheck::verify_after;
use super::recover::approved;
use super::writer::{Decision, WriteCommand};
use crate::events::{DomainEvent, EventKind};
use crate::triage::cut;

/// The largest output excerpt stored, in bytes.
pub const EXCERPT_BYTES: usize = 4 * 1024;

/// Starts running `action` as a task in `running`, stopping it on shutdown. `pool` holds
/// the catch-up runs to a limit; an approval that arrives as an event passes `None`.
fn start(
    running: &mut JoinSet<()>,
    shared: &Arc<Shared>,
    action: ActionId,
    pool: Option<Arc<Semaphore>>,
) {
    let shared = shared.clone();
    running.spawn(async move {
        let stop = shared.deps.shutdown.clone();
        tokio::select! {
            () = stop.cancelled() => {}
            () = run(&shared, action, pool) => {}
        }
    });
}

pub(crate) async fn serve(shared: Arc<Shared>, mut events: broadcast::Receiver<DomainEvent>) {
    let shutdown = shared.deps.shutdown.clone();
    let mut running = JoinSet::new();
    // Approvals made before this hub session started have no event left on the bus,
    // so the store is read once here for actions still waiting to run (ADR 0014).
    catch_up(&mut running, &shared).await;
    loop {
        // Forget finished runs, so the set holds only work still in flight.
        while running.try_join_next().is_some() {}
        let event = tokio::select! {
            () = shutdown.cancelled() => break,
            event = events.recv() => event,
        };
        match event {
            Ok(DomainEvent {
                kind: EventKind::ActionApproved { action },
                ..
            }) => start(&mut running, &shared, action, None),
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                metrics::counter!("domovoi_event_bus_lagged_total", "consumer" => "actions")
                    .increment(missed);
                tracing::warn!(
                    missed,
                    "the action executor missed events; reloading approvals"
                );
                catch_up(&mut running, &shared).await;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    running.shutdown().await;
}

/// The most catch-up runs allowed past the host wait to run at the same time. Every
/// approval the store lists is still started, but each waits for its host before taking
/// one of these slots, so a restart with many approvals does not flood an agent that has
/// just come back. Approvals that arrive as events are not held to this.
pub(crate) const MAX_CATCH_UP_RUNS: usize = 8;

/// Starts what the store still says is approved, after start-up or after the event bus
/// fell behind. A listing the store could not finish starts nothing: the actions stay as
/// they are and the next hub start reads them again, rather than running a part of a set
/// that looks complete.
async fn catch_up(running: &mut JoinSet<()>, shared: &Arc<Shared>) {
    let ids = match approved(shared).await {
        Ok(ids) => ids,
        Err(err) => {
            tracing::error!(
                %err,
                "the store would not say what is still approved; the executor starts none of it",
            );
            return;
        }
    };
    // Every approval is started; the shared pool holds the runs themselves to eight at
    // once, and only after each has waited for its host, so a waiting host never keeps
    // another host's run out of the pool. The pool lives in `Shared`, so a reload after
    // the event bus fell behind does not start a second set of eight beside the first.
    for action in ids {
        start(running, shared, action, Some(shared.catch_up_slots.clone()));
    }
}

/// Marks an action as running until dropped.
pub(crate) struct Claim<'a> {
    running: &'a Mutex<BTreeSet<ActionId>>,
    id: ActionId,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        lock(self.running).remove(&self.id);
    }
}

/// Claims `id` for one run, or `None` while another run of it is in flight: the event
/// path and the catch-up after missed events can both reach the same approved action, and
/// it must reach the agent once.
pub(crate) fn claim(running: &Mutex<BTreeSet<ActionId>>, id: ActionId) -> Option<Claim<'_>> {
    let fresh = lock(running).insert(id);
    fresh.then_some(Claim { running, id })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Queues one transition for the writer task and waits for its answer (ADR 0019). The
/// writer applies it, audits it and logs why it refused, so here a run only stops.
pub(crate) async fn ask(
    shared: &Shared,
    id: ActionId,
    what: &str,
    command: WriteCommand,
    answer: oneshot::Receiver<Decision>,
) -> Option<ActionRequest> {
    if shared.writer.tx.send(command).await.is_err() {
        tracing::debug!(action = %id, "the action service is closing; {what} did not reach the writer");
        return None;
    }
    match answer.await {
        Ok(result) => result.ok(),
        Err(_) => {
            tracing::debug!(action = %id, "the writer stopped before answering about {what}");
            None
        }
    }
}

/// Queues `Dispatch` and answers with the action as the writer left it: `Dispatched`
/// when the plan still matches, `Rejected` when it no longer does (INV-4).
async fn ask_dispatch(shared: &Shared, id: ActionId, plan: PlanHash) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "a dispatch",
        WriteCommand::Dispatch { id, plan, reply },
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

/// Runs action `id` if it is still approved. The freshness check, the wait for the host,
/// the loop guard and the re-validation are checked here, but every change to the action
/// goes through the writer task, which answers with the action as it left it.
///
/// The order matters after a restart: nothing may be dispatched until the host's agent is
/// known and the approval and incident still stand, and a catch-up run holds a pool permit
/// only once it is actually ready to run.
pub(crate) async fn run(shared: &Shared, id: ActionId, pool: Option<Arc<Semaphore>>) {
    let Some(_claim) = claim(&shared.running, id) else {
        tracing::debug!(action = %id, "already running");
        return;
    };
    let deps = &shared.deps;
    let action = match deps.actions.get(id).await {
        Ok(Some(action)) if action.status() == &ActionStatus::Approved => action,
        Ok(_) => return,
        Err(err) => {
            tracing::error!(%err, action = %id, "could not load an approved action");
            return;
        }
    };
    // The wait starts here, not at the approval: the agent gets its whole `offline_wait`
    // after the hub restarts, however old the approval is.
    let started = deps.clock.now();
    if !preflight::still_current(shared, id, &action, started).await {
        return;
    }
    match host_wait::wait_for_host(shared, action.host(), started).await {
        HostWait::Known => {}
        HostWait::Incompatible => {
            let _ = preflight::ask_reject(
                shared,
                id,
                "agent upgrade required: the agent speaks another protocol version".to_owned(),
                false,
            )
            .await;
            return;
        }
        HostWait::TimedOut => {
            let _ = preflight::ask_reject(shared, id, "agent offline".to_owned(), false).await;
            return;
        }
    }
    // Held until this run ends, so the catch-up pool limits runs that are ready to run
    // rather than ones still waiting for a host. The event path passes no pool.
    let _permit = match &pool {
        Some(pool) => match pool.clone().acquire_owned().await {
            Ok(permit) => Some(permit),
            Err(_) => return,
        },
        None => None,
    };
    let now = deps.clock.now();
    // Waiting may have taken an hour: the approval and the incident are asked again
    // before anything is sent.
    if !preflight::still_current(shared, id, &action, now).await {
        return;
    }
    let (skill, args) = match preflight::revalidate(shared, &action) {
        Ok(found) => found,
        Err(reason) => {
            let _ = preflight::ask_reject(shared, id, reason, false).await;
            return;
        }
    };
    // The slot is claimed here, before the plan is even recomputed, so two catch-up runs
    // of the same skill cannot both read the same count and both pass (ADR 0019's writer
    // answers the dispatch, but the guard is this task's own bookkeeping).
    if let Some(reason) = preflight::reserve_dispatch(shared, &action, now) {
        let _ = preflight::ask_reject(shared, id, reason, true).await;
        return;
    }
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &args);
    // The gate closes the window between the write that says `Dispatched` and the frame
    // reaching the agent's queue: reconciliation takes it before it lists what is pending,
    // so a run that has not been handed over yet can never be in that list.
    let gate = shared.dispatch_gate(action.host()).await;
    let Some(dispatched) = ask_dispatch(shared, id, plan).await else {
        return;
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
                "the dispatch recorded no journal epoch; the run is not sent",
            );
            return;
        }
        _ => {
            tracing::debug!(action = %id, "the plan the agent has now is not the approved one");
            return;
        }
    };
    // The writer's answer is not proof the row is still dispatched: an outside decision may
    // have settled it while this run held the gate. Re-read before anything is sent.
    let current = match deps.actions.get(id).await {
        Ok(Some(current)) => current,
        Ok(None) => {
            tracing::debug!(action = %id, "the action vanished before it could be sent");
            return;
        }
        Err(err) => {
            tracing::warn!(%err, action = %id, "could not re-read the action before sending it");
            return;
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
        return;
    }
    let deadline = deps
        .clock
        .now()
        .checked_add(skill.timeout)
        .and_then(|t| t.checked_add(shared.settings.dispatch_grace))
        .unwrap_or(now);
    let dispatch = SkillDispatch {
        action: id,
        skill: skill_name,
        args,
        plan_hash: plan,
    };
    let handoff = match deps
        .link
        .start_skill(action.host(), dispatch, epoch, deadline)
        .await
    {
        Ok(handoff) => handoff,
        Err(err) => {
            // Nothing reached the agent. The row stays dispatched; a later reconciliation
            // pass settles what the journal says about it.
            tracing::warn!(
                action = %id,
                %err,
                "could not hand the run to the agent; it stays dispatched",
            );
            return;
        }
    };
    drop(gate);
    match deps
        .link
        .skill_result(action.host(), handoff, deadline)
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
                verify_after(shared, &action, &skill).await
            } else {
                None
            };
            let _ = ask_complete(shared, id, outcome, Some(record), delay).await;
        }
        Err(LinkError::Rejected(reason)) => {
            // A terminal answer from the agent: it refused the run or reported a failure.
            let _ = ask_complete(shared, id, Err(reason), None, None).await;
        }
        Err(err) => {
            // The request was handed over and no usable answer came back. The run may have
            // happened, so it is neither a success nor a failure: it stays dispatched until
            // the agent's journal can settle it.
            tracing::warn!(
                action = %id,
                %err,
                "the run's result is unavailable; it stays dispatched for reconciliation",
            );
        }
    }
}

fn run_record(shared: &Shared, output: &SkillOutput) -> RunRecord {
    let mut hash = Sha256::new();
    hash.update(output.stdout.as_bytes());
    hash.update([0u8]);
    hash.update(output.stderr.as_bytes());
    let text = if output.stderr.trim().is_empty() {
        output.stdout.clone()
    } else {
        format!("{}\n--- stderr ---\n{}", output.stdout, output.stderr)
    };
    RunRecord {
        exit_code: output.exit_code,
        argv_hash: hex::encode(output.argv_hash),
        output_sha256: hex::encode(hash.finalize()),
        duration_ms: output.duration_ms,
        truncated: output.truncated,
        excerpt: cut(&shared.deps.redactor.redact(&text), EXCERPT_BYTES),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use domovoi_core::ActionId;

    use super::claim;

    #[test]
    fn an_action_is_never_run_twice_at_once() {
        let running = Mutex::new(BTreeSet::new());
        let id = ActionId::new();
        let first = claim(&running, id).expect("the first run claims it");
        assert!(
            claim(&running, id).is_none(),
            "a catch-up run while the event's run is in flight does nothing"
        );
        assert!(
            claim(&running, ActionId::new()).is_some(),
            "others are not blocked"
        );
        drop(first);
        assert!(
            claim(&running, id).is_some(),
            "free again once the run ends"
        );
    }
}
