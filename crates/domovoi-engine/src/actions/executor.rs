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

use domovoi_core::ports::{SkillDispatch, SkillOutput};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, PlanHash, SkillSummary, ValidatedArgs};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, broadcast, oneshot};
use tokio::task::JoinSet;

use super::Shared;
use super::audit::RunRecord;
use super::freshness;
use super::host_wait::{self, HostWait};
use super::recheck::verify;
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
    // The writer first fails what the previous session left running and then signals
    // `recovered`; waiting for it keeps catch-up from starting a run the recovery is
    // about to fail, or against a loop guard that has not finished reloading.
    let mut recovered = shared.recovered.subscribe();
    let stopping = tokio::select! {
        () = shutdown.cancelled() => true,
        result = recovered.wait_for(|done| *done) => {
            if result.is_err() {
                tracing::warn!("the recovery signal was lost before catch-up could start");
            }
            false
        }
    };
    if stopping {
        running.shutdown().await;
        return;
    }
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

/// Queues a rejection made before dispatch. `blocked` is the loop guard, which also
/// publishes `ActionBlocked` so the person knows why nothing ran.
async fn ask_reject(
    shared: &Shared,
    id: ActionId,
    reason: String,
    blocked: bool,
) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "a rejection",
        WriteCommand::Reject {
            id,
            reason,
            blocked,
            reply,
        },
        answer,
    )
    .await
}

/// Queues the agent's answer for a run.
async fn ask_complete(
    shared: &Shared,
    id: ActionId,
    outcome: Result<(), String>,
    record: Option<RunRecord>,
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
    if !still_current(shared, id, &action, started).await {
        return;
    }
    match host_wait::wait_for_host(shared, action.host(), started).await {
        HostWait::Known => {}
        HostWait::Incompatible => {
            let _ = ask_reject(
                shared,
                id,
                "agent upgrade required: the agent speaks another protocol version".to_owned(),
                false,
            )
            .await;
            return;
        }
        HostWait::TimedOut => {
            let _ = ask_reject(shared, id, "agent offline".to_owned(), false).await;
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
    if !still_current(shared, id, &action, now).await {
        return;
    }
    let (skill, args) = match revalidate(shared, &action) {
        Ok(found) => found,
        Err(reason) => {
            let _ = ask_reject(shared, id, reason, false).await;
            return;
        }
    };
    // The slot is claimed here, before the plan is even recomputed, so two catch-up runs
    // of the same skill cannot both read the same count and both pass (ADR 0019's writer
    // answers the dispatch, but the guard is this task's own bookkeeping).
    if let Some(reason) = reserve_dispatch(shared, &action, now) {
        let _ = ask_reject(shared, id, reason, true).await;
        return;
    }
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &args);
    let Some(action) = ask_dispatch(shared, id, plan).await else {
        return;
    };
    if action.status() != &ActionStatus::Dispatched {
        tracing::debug!(action = %id, "the plan the agent has now is not the approved one");
        return;
    }
    let deadline = deps
        .clock
        .now()
        .checked_add(skill.timeout)
        .and_then(|t| t.checked_add(shared.settings.dispatch_grace))
        .unwrap_or(now);
    let dispatch = SkillDispatch {
        action: action.id(),
        skill: skill.name.clone(),
        args,
        plan_hash: plan,
    };
    match deps.link.run_skill(action.host(), dispatch, deadline).await {
        Ok(output) => {
            let record = run_record(shared, &output);
            let outcome = match output.exit_code {
                Some(0) => Ok(()),
                Some(code) => Err(format!("the skill exited with code {code}")),
                None => Err("the skill was killed".to_owned()),
            };
            let ok = outcome.is_ok();
            let _ = ask_complete(shared, id, outcome, Some(record)).await;
            if ok {
                verify(shared, &action, &skill).await;
            }
        }
        Err(err) => {
            let _ = ask_complete(shared, id, Err(err.to_string()), None).await;
        }
    }
}

/// Takes one loop-guard slot for this action, or says why it cannot run: the skill already
/// used every slot on this host in the last hour. The count and the claim are one lock, so
/// runs that the service started together from the store cannot all read the same count and
/// all pass; the runs a previous hub session made are already in the bookkeeping, which the
/// recovery scan put back from the audit log.
fn reserve_dispatch(shared: &Shared, action: &ActionRequest, now: Timestamp) -> Option<String> {
    let hour_ago = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let mut dispatches = shared
        .dispatches
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let runs = dispatches
        .entry((action.host(), action.skill().to_string()))
        .or_default();
    while runs.front().is_some_and(|t| *t <= hour_ago) {
        runs.pop_front();
    }
    let limit = shared.settings.loop_guard_per_hour;
    if runs.len() >= limit as usize {
        return Some(format!(
            "loop guard: {} already ran {limit} times on this host in the last hour",
            action.skill(),
        ));
    }
    runs.push_back(now);
    None
}

/// The skill as the agent has it now and the stored arguments validated against it, or
/// the reason the action must be rejected before dispatch (INV-4). The rejection itself
/// is queued for the writer, which audits it.
fn revalidate(
    shared: &Shared,
    action: &ActionRequest,
) -> Result<(SkillSummary, ValidatedArgs), String> {
    let skills = shared.deps.link.skills(action.host());
    let found = skills.into_iter().find(|s| &s.name == action.skill());
    match found {
        None => Err("the skill is no longer on the host".to_owned()),
        Some(skill) if skill.risk != action.risk() => {
            Err(format!("the skill's risk changed to {:?}", skill.risk))
        }
        Some(skill) => skill
            .validate_args(action.args())
            .map(|args| (skill, args))
            .map_err(|err| format!("the arguments are no longer valid: {err}")),
    }
}

/// Whether `action` may still be dispatched at `now`, rejecting it with the reason when
/// it may not. A store that cannot say leaves it approved: nothing is sent and the next
/// attempt, or the next hub start, asks again.
async fn still_current(
    shared: &Shared,
    id: ActionId,
    action: &ActionRequest,
    now: Timestamp,
) -> bool {
    match freshness::check(shared, action, now).await {
        Ok(None) => true,
        Ok(Some(old)) => {
            let _ = ask_reject(shared, id, old.reason().to_owned(), false).await;
            false
        }
        Err(err) => {
            tracing::error!(
                %err,
                action = %id,
                "could not check whether the approval is still current; it stays approved",
            );
            false
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
