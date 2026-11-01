//! Runs approved actions: loop guard, re-validation (INV-4), dispatch, verification.
//!
//! Each `ActionApproved` event starts one run. A run reloads the action and does nothing
//! unless it is still `Approved`, so a repeated event never runs a skill twice.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{ActionFilter, Page, Presence, SkillDispatch, SkillOutput};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, PlanHash, Risk, SignalStatus, SkillSummary,
    ValidatedArgs, Verdict,
};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tokio::task::JoinSet;

use super::Shared;
use super::audit::{self, RunRecord};
use crate::events::{DomainEvent, EventKind};
use crate::triage::cut;

/// The largest output excerpt stored, in bytes.
pub const EXCERPT_BYTES: usize = 4 * 1024;

/// How long a verification re-check may take.
pub const RECHECK_DEADLINE: SignedDuration = SignedDuration::from_secs(60);

/// Starts running `action` as a task in `running`, stopping it on shutdown.
fn start(running: &mut JoinSet<()>, shared: &Arc<Shared>, action: ActionId) {
    let shared = shared.clone();
    running.spawn(async move {
        let stop = shared.deps.shutdown.clone();
        tokio::select! {
            () = stop.cancelled() => {}
            () = run(&shared, action) => {}
        }
    });
}

pub(crate) async fn serve(shared: Arc<Shared>, mut events: broadcast::Receiver<DomainEvent>) {
    let shutdown = shared.deps.shutdown.clone();
    let mut running = JoinSet::new();
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
            }) => start(&mut running, &shared, action),
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                metrics::counter!("domovoi_event_bus_lagged_total", "consumer" => "actions")
                    .increment(missed);
                tracing::warn!(
                    missed,
                    "the action executor missed events; reloading approvals"
                );
                for action in approved(&shared).await {
                    start(&mut running, &shared, action);
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    running.shutdown().await;
}

/// Every action still waiting to run, for catching up after missed events.
async fn approved(shared: &Arc<Shared>) -> Vec<ActionId> {
    let filter = ActionFilter {
        status: Some("approved".to_owned()),
        ..ActionFilter::default()
    };
    let mut ids = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = match shared.deps.actions.list(&filter, &page).await {
            Ok(got) => got,
            Err(err) => {
                tracing::warn!(%err, "could not reload approved actions");
                return ids;
            }
        };
        ids.extend(got.items.iter().map(ActionRequest::id));
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return ids,
        }
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

fn save_error(err: &str) {
    tracing::error!(%err, "could not store an action change");
}

/// Runs action `id` if it is still approved.
pub(crate) async fn run(shared: &Shared, id: ActionId) {
    let Some(_claim) = claim(&shared.running, id) else {
        tracing::debug!(action = %id, "already running");
        return;
    };
    let deps = &shared.deps;
    let mut action = match deps.actions.get(id).await {
        Ok(Some(action)) if action.status() == &ActionStatus::Approved => action,
        Ok(_) => return,
        Err(err) => {
            tracing::error!(%err, action = %id, "could not load an approved action");
            return;
        }
    };
    if let Some(reason) = over_loop_guard(shared, &action) {
        block(shared, &mut action, &reason).await;
        return;
    }
    let Some((skill, args)) = revalidate(shared, &mut action).await else {
        return;
    };
    let now = deps.clock.now();
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &args);
    if action.dispatch(plan, now).is_err() {
        return;
    }
    if let ActionStatus::Rejected { reason } = action.status().clone() {
        let entry = audit::rejected(&action, &reason, now);
        if let Err(err) = shared.record(&action, entry).await {
            save_error(&err);
        }
        return;
    }
    remember_dispatch(shared, &action, now);
    if let Err(err) = shared
        .record(&action, audit::dispatched(&action, now))
        .await
    {
        save_error(&err);
        return;
    }
    if !wait_for_agent(shared, &action).await {
        finish(shared, &mut action, None, Err("agent offline".to_owned())).await;
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
            let result = match output.exit_code {
                Some(0) => Ok(()),
                Some(code) => Err(format!("the skill exited with code {code}")),
                None => Err("the skill was killed".to_owned()),
            };
            let ok = result.is_ok();
            finish(shared, &mut action, Some(&record), result).await;
            if ok {
                verify(shared, &mut action, &skill).await;
            }
        }
        Err(err) => finish(shared, &mut action, None, Err(err.to_string())).await,
    }
}

/// `Some(reason)` when this skill already ran too often on this host in the last hour.
fn over_loop_guard(shared: &Shared, action: &ActionRequest) -> Option<String> {
    let now = shared.deps.clock.now();
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
    (runs.len() >= limit as usize).then(|| {
        format!(
            "loop guard: {} already ran {limit} times on this host in the last hour",
            action.skill()
        )
    })
}

fn remember_dispatch(shared: &Shared, action: &ActionRequest, now: Timestamp) {
    shared
        .dispatches
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry((action.host(), action.skill().to_string()))
        .or_default()
        .push_back(now);
}

async fn block(shared: &Shared, action: &mut ActionRequest, reason: &str) {
    let now = shared.deps.clock.now();
    if action.reject(reason.to_owned(), now).is_err() {
        return;
    }
    if let Err(err) = shared
        .record(action, audit::blocked(action, reason, now))
        .await
    {
        save_error(&err);
        return;
    }
    shared
        .publish(
            action,
            EventKind::ActionBlocked {
                action: action.id(),
                reason: reason.to_owned(),
            },
        )
        .await;
}

/// The skill as the agent has it now and the stored arguments validated against it, or
/// `None` after rejecting the action.
async fn revalidate(
    shared: &Shared,
    action: &mut ActionRequest,
) -> Option<(SkillSummary, ValidatedArgs)> {
    let skills = shared.deps.link.skills(action.host());
    let found = skills.into_iter().find(|s| &s.name == action.skill());
    let outcome = match found {
        None => Err("the skill is no longer on the host".to_owned()),
        Some(skill) if skill.risk != action.risk() => {
            Err(format!("the skill's risk changed to {:?}", skill.risk))
        }
        Some(skill) => match skill.validate_args(action.args()) {
            Ok(args) => Ok((skill, args)),
            Err(err) => Err(format!("the arguments are no longer valid: {err}")),
        },
    };
    match outcome {
        Ok(found) => Some(found),
        Err(reason) => {
            let now = shared.deps.clock.now();
            if action.reject(reason.clone(), now).is_ok()
                && let Err(err) = shared
                    .record(action, audit::rejected(action, &reason, now))
                    .await
            {
                save_error(&err);
            }
            None
        }
    }
}

/// Waits up to `offline_wait` for the host's agent. Returns whether it is connected.
async fn wait_for_agent(shared: &Shared, action: &ActionRequest) -> bool {
    let deps = &shared.deps;
    let start = deps.clock.now();
    let give_up = start
        .checked_add(shared.settings.offline_wait)
        .unwrap_or(start);
    loop {
        if matches!(
            deps.link.presence(action.host()),
            Presence::Connected { .. }
        ) {
            return true;
        }
        let now = deps.clock.now();
        if now >= give_up {
            return false;
        }
        let next = now
            .checked_add(shared.settings.presence_poll)
            .unwrap_or(give_up)
            .min(give_up);
        deps.clock.sleep_until(next).await;
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

async fn finish(
    shared: &Shared,
    action: &mut ActionRequest,
    record: Option<&RunRecord>,
    result: Result<(), String>,
) {
    let now = shared.deps.clock.now();
    let reason = result.as_ref().err().cloned();
    if action.complete(result, now).is_err() {
        return;
    }
    let entry = audit::finished(action, record, reason.as_deref(), now);
    if let Err(err) = shared.record(action, entry).await {
        save_error(&err);
    }
}

/// How long to wait before re-checking, or `None` when the action is not verified.
fn verify_delay(
    shared: &Shared,
    skill: &SkillSummary,
    check: &domovoi_core::CheckKey,
) -> Option<SignedDuration> {
    match &skill.verify {
        Some(spec) if spec.recheck => Some(spec.delay),
        Some(_) => None,
        None => (skill.risk == Risk::Write && skill.applies_to.contains(check))
            .then_some(shared.settings.verify_default_delay),
    }
}

async fn verify(shared: &Shared, action: &mut ActionRequest, skill: &SkillSummary) {
    let deps = &shared.deps;
    let Ok(Some(incident)) = deps.incidents.get(action.incident()).await else {
        return;
    };
    let Some(delay) = verify_delay(shared, skill, incident.check()) else {
        return;
    };
    let now = deps.clock.now();
    deps.clock
        .sleep_until(now.checked_add(delay).unwrap_or(now))
        .await;
    let deadline = deps
        .clock
        .now()
        .checked_add(RECHECK_DEADLINE)
        .unwrap_or(now);
    let recheck = deps
        .link
        .run_check(
            action.host(),
            incident.check().clone(),
            incident.target().cloned(),
            deadline,
        )
        .await;
    let (passed, summary) = match recheck {
        Ok(result) => (
            result.status == SignalStatus::Resolved,
            deps.redactor.redact(&result.summary),
        ),
        Err(err) => (false, format!("the re-check could not run: {err}")),
    };
    let now = deps.clock.now();
    if action.verified(passed, now).is_err() {
        return;
    }
    let entry = audit::verification(action, passed, &summary, now);
    if let Err(err) = shared.record(action, entry).await {
        save_error(&err);
        return;
    }
    if passed
        && let Err(err) = deps
            .manager
            .set_verdict(action.incident(), Verdict::Fixed)
            .await
    {
        tracing::warn!(%err, "could not mark the incident fixed");
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
