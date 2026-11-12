//! One admission gate for every model-backed triage task: triage runs, incident follow-up
//! questions and general chat all pass through [`admit`] before a model call starts, so the
//! hourly model cap and the monthly budget are checked in one place.
//!
//! Questions and chat may not use the last `reserved_for_incidents` runs of the rolling
//! hour: that many runs always stay free for triage. Triage itself may use the whole hour.

use std::collections::VecDeque;

use crate::triage::Shared;
use crate::triage::run::limits;

use jiff::{SignedDuration, Timestamp, tz::TimeZone};

/// Which kind of task is asking to start. Triage may use the whole hour; a question
/// leaves the reserved runs alone.
pub(crate) enum Purpose {
    Triage,
    Question,
}

/// Why admission refused a task, and when a retry could fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The monthly model budget is spent; nothing retries it before the next month.
    BudgetSpent,
    /// The hourly run limit is reached. `retry_at` is when an admissible slot frees, or
    /// `None` when the limit cannot admit this task at all (a zero hour, or a hour whose
    /// whole capacity is reserved for triage).
    Hourly { retry_at: Option<Timestamp> },
}

/// The start of the rolling hour the hourly run limit counts.
fn hour_ago(now: Timestamp) -> Timestamp {
    now.checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now)
}

/// When a run that started at `oldest` frees its slot, or `None` when the arithmetic
/// leaves the timestamp range.
pub(crate) fn slot_after(oldest: Timestamp) -> Option<Timestamp> {
    oldest
        .checked_add(SignedDuration::from_hours(1))
        .and_then(|at| at.checked_add(SignedDuration::from_secs(1)))
        .ok()
}

/// Whether the monthly model budget is spent, reading the model spending for the current
/// calendar month (UTC). A failed read is not a spent budget: triage still runs.
pub(crate) async fn budget_spent(shared: &Shared) -> bool {
    let Some(budget) = shared.settings.monthly_budget_micros else {
        return false;
    };
    let month = shared.deps.clock.now().to_zoned(TimeZone::UTC).date();
    match shared.deps.usage.month_total(month).await {
        Ok(total) => total.cost_micros >= budget,
        Err(err) => {
            tracing::warn!(%err, "could not read model spending; still checking admission");
            false
        }
    }
}

/// How many recorded run starts have already left the rolling hour.
fn expired_runs(starts: &VecDeque<Timestamp>, since: Timestamp) -> usize {
    starts.iter().take_while(|t| **t <= since).count()
}

/// Whether a task may start now, recording its start when it may.
pub(crate) async fn admit(shared: &Shared, purpose: Purpose) -> Result<(), Refusal> {
    if budget_spent(shared).await {
        return Err(Refusal::BudgetSpent);
    }
    let now = shared.deps.clock.now();
    let since = hour_ago(now);
    let mut window = limits(shared);
    while window.starts.front().is_some_and(|t| *t <= since) {
        window.starts.pop_front();
    }
    let capacity = shared.settings.per_hour_runs as usize;
    let reserved = shared.settings.reserved_for_incidents as usize;
    let room = match purpose {
        Purpose::Triage => capacity,
        Purpose::Question => capacity.saturating_sub(reserved),
    };
    if window.starts.len() >= room {
        return Err(Refusal::Hourly {
            retry_at: match room {
                0 => None,
                // Triage may have used the runs kept free for it, so more than one start
                // can have to leave the hour before this task fits: the start whose
                // expiry frees the slot is the one at `len - room`.
                _ => window
                    .starts
                    .get(window.starts.len() - room)
                    .and_then(|t| slot_after(*t)),
            },
        });
    }
    window.starts.push_back(now);
    Ok(())
}

/// Free run slots in the current hour.
pub(crate) fn free_slots(shared: &Shared, now: Timestamp) -> usize {
    let since = hour_ago(now);
    let window = limits(shared);
    let used = window.starts.len() - expired_runs(&window.starts, since);
    let capacity = shared.settings.per_hour_runs as usize;
    capacity.saturating_sub(used)
}

/// When the oldest run still in the window frees its slot, if any run is.
pub(crate) fn next_slot(shared: &Shared) -> Option<Timestamp> {
    let now = shared.deps.clock.now();
    let since = hour_ago(now);
    let window = limits(shared);
    let expired = expired_runs(&window.starts, since);
    window.starts.get(expired).and_then(|t| slot_after(*t))
}
