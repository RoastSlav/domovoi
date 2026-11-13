//! What a service that has only just started reads back from the store, so a hub that
//! restarted neither loses what a person decided nor invents a run (ADR 0014): actions
//! still marked **Approved** are found here for the executor to start, actions still
//! marked **Dispatched** are failed because their answer went to the session that is
//! gone, and the loop guard's memory of the last hour is reloaded from the audit log.
//! A scan the store will not finish is an error rather than a shorter list: the work it
//! was meant to do stays undone until the hub starts again, instead of looking complete.

use domovoi_core::ActionChange;
use domovoi_core::ports::{
    ActionFilter, ActionGuard, AuditEvent, AuditFilter, CommitError, Page, StoreError,
};
use domovoi_core::{ActionId, ActionRequest};

use super::Publish;
use super::Shared;
use super::audit;

use jiff::SignedDuration;

/// How many tries a start-of-service scan gets before it gives up. A page that fails
/// is retried by starting the scan over from the first page, so a store hiccup is never
/// read as a listing that finished; the retries are immediate, nothing sleeps. After
/// this many tries the scan reports an error and the caller leaves those actions as the
/// store has them.
pub(crate) const SCAN_ATTEMPTS: usize = 3;

/// Why an action that was running when the hub stopped is failed. Nothing is known
/// about the run: the agent's answer went to the session that is gone.
pub(crate) const RESTART_REASON: &str =
    "the hub restarted while this ran; check the host before running it again";

/// Every action matching `filter`, paged to the end and read again from the start if a
/// page fails. `Err` means the store would not finish the listing: whoever started work
/// from it must not treat what it gave as the whole set, so leftovers wait for the next
/// hub start rather than being half handled.
pub(crate) async fn scan(
    shared: &Shared,
    filter: &ActionFilter,
) -> Result<Vec<ActionRequest>, StoreError> {
    let mut attempt = 1;
    let mut found = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = match shared.deps.actions.list(filter, &page).await {
            Ok(got) => got,
            Err(err) if attempt < SCAN_ATTEMPTS => {
                tracing::warn!(
                    %err,
                    attempt,
                    "the action listing failed; starting the whole scan over",
                );
                attempt += 1;
                found = Vec::new();
                page = Page::first(Page::MAX_LIMIT);
                continue;
            }
            Err(err) => {
                tracing::error!(
                    %err,
                    tries = SCAN_ATTEMPTS,
                    "the store would not finish listing actions; the scan is left incomplete",
                );
                return Err(err);
            }
        };
        found.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(found),
        }
    }
}

/// Audit rows matching `filter`, read the same way [`scan`] reads actions: paged to the
/// end, retried from the start when a page fails.
async fn scanned(shared: &Shared, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
    let mut attempt = 1;
    let mut found = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = match shared.deps.audit.list(filter, &page).await {
            Ok(got) => got,
            Err(err) if attempt < SCAN_ATTEMPTS => {
                tracing::warn!(
                    %err,
                    attempt,
                    "the audit listing failed; starting the whole scan over",
                );
                attempt += 1;
                found = Vec::new();
                page = Page::first(Page::MAX_LIMIT);
                continue;
            }
            Err(err) => {
                tracing::error!(
                    %err,
                    tries = SCAN_ATTEMPTS,
                    "the audit log would not finish listing dispatches; the scan is left incomplete",
                );
                return Err(err);
            }
        };
        found.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(found),
        }
    }
}

/// The action an audit row was written for, from its `action:<id>` subject. Rows about
/// anything else have no action behind them.
fn audited(subject: &str) -> Option<ActionId> {
    let (kind, id) = subject.split_once(':')?;
    if kind != "action" {
        return None;
    }
    id.parse().ok()
}

/// Every action still waiting to run, for the executor to start when the service starts
/// and again when the event bus falls behind. `Err` says the store would not list them,
/// which is not the same as there being none: the executor starts nothing from a scan it
/// could not finish and says so.
pub(crate) async fn approved(shared: &Shared) -> Result<Vec<ActionId>, StoreError> {
    let filter = ActionFilter {
        status: Some("approved".to_owned()),
        ..ActionFilter::default()
    };
    scan(shared, &filter)
        .await
        .map(|found| found.iter().map(ActionRequest::id).collect())
}

/// Puts the loop guard back to what it was for the hour it has now: the dispatch times
/// the audit log holds for the current rolling hour, keyed by host and skill the way the
/// guard keys its own runs. The audit log has the instant and the store has the action
/// behind it, so neither is guessed at. Without this a restart forgets what already ran
/// and lets the same skill through once more per leftover slot, so it runs before any
/// new run is checked against the guard.
pub(crate) async fn dispatch_history(shared: &Shared) -> Result<usize, StoreError> {
    let now = shared.deps.clock.now();
    let hour_ago = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let filter = AuditFilter {
        kind: Some(audit::DISPATCHED.to_owned()),
        since: Some(hour_ago),
        ..AuditFilter::default()
    };
    let rows = match scanned(shared, &filter).await {
        Ok(rows) => rows,
        Err(err) => return Err(err),
    };
    // The key and the instant are read per row here, and only the short walk over what
    // was read takes the guard, so the store is never waited on with a lock in hand.
    let mut recent = Vec::new();
    for entry in rows {
        let Some(id) = audited(&entry.subject) else {
            continue;
        };
        let found = match shared.deps.actions.get(id).await {
            Ok(Some(action)) => Some(action),
            Ok(None) => {
                tracing::debug!(
                    action = %id,
                    "a dispatch in the audit log has no action behind it now",
                );
                None
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    action = %id,
                    "could not load the action a dispatch belongs to",
                );
                None
            }
        };
        if let Some(action) = found {
            recent.push(((action.host(), action.skill().to_string()), entry.at));
        }
    }
    let mut guard = shared
        .dispatches
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Rows come newest first, the guard's history oldest first, so sort once here rather
    // than per key; then the guard's own hour window trims anything that has aged out.
    recent.sort();
    let reloaded = recent.len();
    for (key, at) in recent {
        let runs = guard.entry(key).or_default();
        while runs.front().is_some_and(|t| *t <= hour_ago) {
            runs.pop_front();
        }
        runs.push_back(at);
    }
    Ok(reloaded)
}

/// Fails every action the store says was running and returns how many. The store picks
/// them out, so this reads only leftovers, and the guarded write keeps a decision that
/// came in the meantime from being overwritten. `Err` means it could not get the whole
/// list: those actions stay as the store has them and the next hub start reads them again.
pub(crate) async fn fail_abandoned(shared: &Shared) -> Result<usize, StoreError> {
    let filter = ActionFilter {
        status: Some("dispatched".to_owned()),
        ..ActionFilter::default()
    };
    let leftovers = scan(shared, &filter).await?;
    let mut failed = 0;
    for mut action in leftovers {
        let now = shared.deps.clock.now();
        let ActionChange { from, .. } = match action.complete(Err(RESTART_REASON.to_owned()), now) {
            Ok(change) => change,
            Err(err) => {
                tracing::debug!(
                    action = %action.id(),
                    %err,
                    "an action left running already has an outcome",
                );
                continue;
            }
        };
        match shared
            .commit_action(
                &action,
                ActionGuard::Status(from.name().to_owned()),
                audit::finished(&action, None, Some(RESTART_REASON), now),
                now,
                None,
                Publish::ActionChanged,
            )
            .await
        {
            Ok(()) => failed += 1,
            Err(CommitError::Conflict { .. }) => tracing::debug!(
                action = %action.id(),
                "someone decided this action meanwhile; the restart leaves it as they left it",
            ),
            Err(err) => tracing::warn!(
                %err,
                action = %action.id(),
                "could not fail an action left running",
            ),
        }
    }
    Ok(failed)
}
