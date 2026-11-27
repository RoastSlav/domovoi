//! Carrying on after a model outage: the investigations that failed while the model was
//! unreachable are started again, one at a time, once the model answers.

use std::sync::atomic::{AtomicBool, Ordering};

use domovoi_core::ports::{IncidentFilter, LlmMessage, LlmRequest, Page, Part, Role};
use domovoi_core::{IncidentStatus, OperationKind, OperationStatus};
use jiff::SignedDuration;

use super::Runtime;
use crate::advisories::triage_degraded;
use crate::operations::handle::OperationHandle;
use crate::triage::run::succeeded;

/// How far back a failed investigation is still worth starting again.
const RECOVER_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// How long to wait for one restarted investigation before the next one starts.
const ONE_AT_A_TIME: SignedDuration = SignedDuration::from_mins(10);
/// How often the model is asked while a stored notice says it was down.
const PROBE_EVERY: SignedDuration = SignedDuration::from_mins(5);
/// How often a restarted investigation is looked at.
const POLL: SignedDuration = SignedDuration::from_secs(5);

/// Starts failed investigations again whenever the model answers after an outage, and probes
/// the model after a start that found the outage notice still open.
pub(super) async fn run_recoverer(handle: OperationHandle) {
    let shared = handle.shared.clone();
    let Some(runtime) = shared.investigate.clone() else {
        return;
    };
    let shutdown = shared.deps.shutdown.clone();
    let busy = AtomicBool::new(false);
    let mut probing = Box::pin(probe_until_answered(&runtime));
    let mut probe_done = false;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            was_open = &mut probing, if !probe_done => {
                probe_done = true;
                if was_open && !busy.swap(true, Ordering::SeqCst) {
                    sweep(&handle, &runtime).await;
                    busy.store(false, Ordering::SeqCst);
                }
                pull_parked_retries(&handle).await;
            }
            () = runtime.triage.pull_parked.notified() => {
                pull_parked_retries(&handle).await;
            }
            () = runtime.triage.recovered.notified() => {
                if !busy.swap(true, Ordering::SeqCst) {
                    sweep(&handle, &runtime).await;
                    busy.store(false, Ordering::SeqCst);
                }
            }
        }
    }
}

/// Gap between two parked retries pulled forward, so they run one at a time and the hourly
/// limit and monthly budget are checked at each start.
const PULL_GAP: SignedDuration = SignedDuration::from_mins(1);

/// Makes parked retries due now, one every [`PULL_GAP`], until none is left waiting.
pub(super) async fn pull_parked_retries(handle: &OperationHandle) {
    let deps = &handle.shared.deps;
    loop {
        match deps.effects.pull_forward_retry(deps.clock.now()).await {
            Ok(0) => return,
            Ok(_) => {
                tracing::info!("the model answers again; a parked triage retry runs now");
            }
            Err(err) => {
                tracing::warn!(%err, "could not pull a parked retry forward");
                return;
            }
        }
        let Ok(next) = deps.clock.now().checked_add(PULL_GAP) else {
            return;
        };
        tokio::select! {
            () = deps.shutdown.cancelled() => return,
            () = deps.clock.sleep_until(next) => {}
        }
    }
}

/// While the stored notice that the model is unreachable is open, asks the model one tiny
/// question every few minutes; the first answer resolves the notice. Returns when it did, or
/// at once when no notice was open at the start.
async fn probe_until_answered(runtime: &Runtime) -> bool {
    let triage = &runtime.triage;
    let deps = &triage.deps;
    let id = triage_degraded("", deps.clock.now()).id;
    let open = deps
        .advisories
        .list_open()
        .await
        .is_ok_and(|open| open.iter().any(|a| a.id == id));
    // No notice is open: a retry parked before this start may still wait for a model that
    // works now. One question tells; on an answer the caller pulls parked retries forward,
    // on no answer the retry keeps its own time.
    if !open {
        let asked = deps.primary.complete(probe_request()).await;
        if asked.is_err() {
            std::future::pending::<()>().await;
        }
        return false;
    }
    loop {
        let asked = deps.primary.complete(probe_request()).await;
        match asked {
            Ok(_) => {
                tracing::info!("the model answers again; the unreachable notice is cleared");
                succeeded(triage).await;
                // `succeeded` resolves the stored notice; the sweep that follows is ours.
                if let Err(err) = deps.advisories.resolve(id, deps.clock.now()).await {
                    tracing::debug!(%err, "no open triage advisory to resolve");
                }
                return true;
            }
            Err(err) => {
                tracing::info!(%err, "the model still does not answer");
                let next = deps.clock.now().checked_add(PROBE_EVERY);
                match next {
                    Ok(at) => deps.clock.sleep_until(at).await,
                    Err(_) => return false,
                }
            }
        }
    }
}

/// Starts one new investigation for each open incident whose latest investigation failed in
/// the last day, one after the other: the next starts when the one before ends, or after ten
/// minutes. The hourly limit and the monthly budget still apply to each start.
pub(super) async fn sweep(handle: &OperationHandle, runtime: &Runtime) {
    let deps = &runtime.triage.deps;
    let clock = &deps.clock;
    let now = clock.now();
    let rules = match deps.acks.list_rules().await {
        Ok(rules) => rules,
        Err(err) => {
            tracing::warn!(%err, "could not read known conditions; not restarting investigations");
            return;
        }
    };
    let filter = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    let mut page = Page::first(Page::MAX_LIMIT);
    let mut restarted: u32 = 0;
    loop {
        let got = match deps.incidents.list(&filter, &page).await {
            Ok(got) => got,
            Err(err) => {
                tracing::warn!(%err, "could not list incidents to restart investigations");
                return;
            }
        };
        for incident in got.items {
            if incident.status() != IncidentStatus::Open {
                continue;
            }
            let covered = rules.iter().any(|rule| {
                rule.covers_incident(
                    incident.host(),
                    incident.check(),
                    incident.target().map(|t| t.as_str()),
                    now,
                )
            });
            if covered {
                continue;
            }
            let Ok(operations) = handle
                .shared
                .deps
                .operations
                .for_incident(incident.id())
                .await
            else {
                continue;
            };
            let mut investigations: Vec<_> = operations
                .iter()
                .filter(|op| op.kind == OperationKind::Investigate)
                .collect();
            investigations.sort_by_key(|op| (op.created_at, op.id));
            let Some(latest) = investigations.last() else {
                continue;
            };
            let recent = latest
                .ended_at
                .and_then(|at| at.checked_add(RECOVER_WINDOW).ok())
                .is_some_and(|until| until > now);
            if latest.status != OperationStatus::Failed || !recent {
                continue;
            }
            if restarted >= u32::from(runtime.settings.catch_up_limit) {
                return;
            }
            restarted += 1;
            tracing::info!(incident = %incident.id(), "starting a failed investigation again; the model answers");
            match handle.investigate(&incident, false).await {
                Ok(id) => wait_for_end(handle, id).await,
                Err(err) => {
                    tracing::warn!(%err, incident = %incident.id(), "could not restart an investigation");
                }
            }
            if deps.shutdown.is_cancelled() {
                return;
            }
        }
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return,
        }
    }
}

/// Waits until `operation` is over, is waiting for someone else (a person, an agent), or ten
/// minutes passed.
async fn wait_for_end(handle: &OperationHandle, operation: domovoi_core::OperationId) {
    let deps = &handle.shared.deps;
    let start = deps.clock.now();
    let limit = start.checked_add(ONE_AT_A_TIME).unwrap_or(start);
    loop {
        match deps.operations.get(operation).await {
            Ok(Some(found)) => {
                let status = found.value.status;
                if status.is_terminal()
                    || matches!(
                        status,
                        OperationStatus::WaitingApproval
                            | OperationStatus::WaitingUser
                            | OperationStatus::WaitingAgent
                    )
                {
                    return;
                }
            }
            _ => return,
        }
        let now = deps.clock.now();
        if now >= limit || deps.shutdown.is_cancelled() {
            return;
        }
        let next = now.checked_add(POLL).unwrap_or(limit).min(limit);
        tokio::select! {
            () = deps.shutdown.cancelled() => return,
            () = deps.clock.sleep_until(next) => {}
        }
    }
}

fn probe_request() -> LlmRequest {
    LlmRequest {
        system: "You are a connection test. Reply with OK.".to_owned(),
        messages: vec![LlmMessage {
            role: Role::User,
            parts: vec![Part::Text("Reply with OK.".to_owned())],
        }],
        tools: Vec::new(),
        max_output_tokens: 16,
    }
}
