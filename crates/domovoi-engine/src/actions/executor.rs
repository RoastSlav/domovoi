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

use domovoi_core::{ActionId, ActionRequest, ActionStatus};
use tokio::sync::{Semaphore, broadcast, oneshot};
use tokio::task::JoinSet;

use super::Shared;
use super::host_wait::{self, HostWait};
use super::preflight;
use super::recover::approved;
use super::writer::{Decision, WriteCommand};
use crate::events::{DomainEvent, EventKind};

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
    // before anything is sent. The incident read is kept for the v2 target, so the plan
    // and the freshness check are computed from one read.
    let Some(incident) = preflight::current_incident(shared, id, &action, now).await else {
        return;
    };
    let Some(snapshot) = deps.link.snapshot(action.host()) else {
        let _ = preflight::ask_reject(
            shared,
            id,
            "no authenticated agent session is available".to_owned(),
            false,
        )
        .await;
        return;
    };
    let (skill, args) = match preflight::revalidate(&snapshot.skills, &action) {
        Ok(found) => found,
        Err(reason) => {
            let _ = preflight::ask_reject(shared, id, reason, false).await;
            return;
        }
    };
    // Everything from the loop guard on is the one execution boundary: it claims the loop
    // guard's slot, recomputes the approval-family hash, commits the single dispatch, sends
    // the request and settles the answer. The reads above are handed over so the plan, the
    // guard and the request all describe the same incident and the same agent session.
    let authorized = match super::execute::AuthorizedExecution::of(action) {
        Ok(authorized) => authorized,
        Err(err) => {
            tracing::warn!(action = %id, %err, "the action cannot be dispatched");
            return;
        }
    };
    if let Err(err) =
        super::execute::execute(shared, authorized, skill, args, snapshot, incident).await
    {
        tracing::debug!(action = %id, %err, "the run was not dispatched");
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
