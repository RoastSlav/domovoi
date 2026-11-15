//! One admission gate for every model-backed triage task: triage runs, incident follow-up
//! questions and general chat all pass through [`admit`] before a model call starts, so the
//! hourly model cap and the monthly budget are checked in one place.
//!
//! The rolling hour is persisted in [`AdmissionRepo`], so a restart does not reset it.
//! Questions and chat may not use the last `reserved_for_incidents` runs of the hour: that
//! many runs always stay free for triage. Triage itself may use the whole hour. Every read
//! error refuses the call: an unreadable limit is never treated as room.

use domovoi_core::ports::{AdmissionOutcome, AdmissionPurpose, StoreError};
use jiff::{SignedDuration, Timestamp, tz::TimeZone};

use crate::triage::Shared;
use crate::triage::run::provider_name;

/// The length of the rolling window the hourly run limit counts.
pub(crate) const WINDOW: SignedDuration = SignedDuration::from_hours(1);

/// How long a triage that could not be admitted because limits were unreadable waits.
pub(crate) const ADMISSION_RETRY: SignedDuration = SignedDuration::from_mins(5);

/// Which kind of task is asking to start. Triage may use the whole hour; a question
/// leaves the reserved runs alone.
pub(crate) enum Purpose {
    Triage,
    Question,
}

impl Purpose {
    fn stored(self) -> AdmissionPurpose {
        match self {
            Self::Triage => AdmissionPurpose::Triage,
            Self::Question => AdmissionPurpose::Question,
        }
    }
}

/// Why admission refused a task, and when a retry could fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The monthly model budget is spent; nothing retries it before the next month.
    BudgetSpent,
    /// A provider in the chain has no price while a monthly budget is set, so its turns
    /// could not be counted. `provider` names it.
    Unpriced { provider: String },
    /// The hourly run limit is reached. `retry_at` is when an admissible slot frees, or
    /// `None` when the limit cannot admit this task at all (a zero hour, or a hour whose
    /// whole capacity is reserved for triage).
    Hourly { retry_at: Option<Timestamp> },
    /// The budget or the persisted window could not be read: nothing may start.
    Unverifiable,
}

/// The sentence a task shows when a provider cannot be priced under a monthly budget.
pub(crate) fn unpriced_text(provider: &str) -> String {
    format!(
        "the monthly model budget is set but {provider} has no price, so spending cannot be \
         measured; set a price for it (0 for a free model) or remove the monthly budget"
    )
}

/// The first provider in the chain that cannot price its turns while a monthly budget is
/// set. Checked before any request, so an unpriced provider is never called and its usage
/// is never written down as free.
fn unpriced(shared: &Shared) -> Option<String> {
    shared.settings.monthly_budget_micros?;
    shared
        .chain()
        .into_iter()
        .find(|provider| !provider.prices_usage())
        .map(|provider| provider_name(&provider))
}

/// The start of the rolling hour the hourly run limit counts.
fn hour_ago(now: Timestamp) -> Timestamp {
    now.checked_sub(WINDOW).unwrap_or(now)
}

/// When a run that started at `oldest` frees its slot, or `None` when the arithmetic
/// leaves the timestamp range.
pub(crate) fn slot_after(oldest: Timestamp) -> Option<Timestamp> {
    oldest
        .checked_add(WINDOW)
        .and_then(|at| at.checked_add(SignedDuration::from_secs(1)))
        .ok()
}

/// Counts a refusal and returns it.
fn refuse(refusal: Refusal) -> Refusal {
    let reason = match refusal {
        Refusal::BudgetSpent => "budget",
        Refusal::Unpriced { .. } => "unpriced",
        Refusal::Hourly { .. } => "hourly",
        Refusal::Unverifiable => "unverifiable",
    };
    crate::telemetry::record_admission_rejection(reason);
    refusal
}

/// Whether a task may start now, recording its start when it may.
pub(crate) async fn admit(shared: &Shared, purpose: Purpose) -> Result<(), Refusal> {
    if let Some(budget) = shared.settings.monthly_budget_micros {
        let month = shared.deps.clock.now().to_zoned(TimeZone::UTC).date();
        match shared.deps.usage.month_total(month).await {
            Ok(total) => {
                // A response whose price is unknown cannot be counted, so the month's spend
                // is not measurable; refusing is the only safe answer.
                if total.unpriced_responses > 0 {
                    tracing::warn!(
                        unpriced = total.unpriced_responses,
                        "a successful response has no known price, so the monthly spend cannot be measured; refusing the model call",
                    );
                    return Err(refuse(Refusal::Unverifiable));
                }
                if total.cost_micros >= budget {
                    return Err(refuse(Refusal::BudgetSpent));
                }
            }
            Err(err) => {
                let err = shared.deps.redactor.redact(&err.to_string());
                tracing::warn!(%err, "could not read model spending; refusing the model call");
                return Err(refuse(Refusal::Unverifiable));
            }
        }
    }
    if let Some(provider) = unpriced(shared) {
        return Err(refuse(Refusal::Unpriced { provider }));
    }
    let now = shared.deps.clock.now();
    let capacity = shared.settings.per_hour_runs;
    let reserved = shared.settings.reserved_for_incidents;
    let room = match purpose {
        Purpose::Triage => capacity,
        Purpose::Question => capacity.saturating_sub(reserved),
    };
    match shared
        .deps
        .admission
        .admit(purpose.stored(), room, WINDOW, now)
        .await
    {
        Ok(AdmissionOutcome::Admitted) => Ok(()),
        Ok(AdmissionOutcome::Full { starts }) => {
            let retry_at = match room {
                0 => None,
                // Triage may have used the runs kept free for it, so more than one start
                // can have to leave the hour before this task fits: the start whose
                // expiry frees the slot is the one at `len - room`.
                _ => starts
                    .len()
                    .checked_sub(room as usize)
                    .and_then(|index| starts.get(index))
                    .and_then(|t| slot_after(*t)),
            };
            Err(refuse(Refusal::Hourly { retry_at }))
        }
        Err(err) => {
            let err = shared.deps.redactor.redact(&err.to_string());
            tracing::warn!(%err, "could not read the model run window; refusing the model call");
            Err(refuse(Refusal::Unverifiable))
        }
    }
}

/// Free run slots in the current hour. A read error is the caller's to refuse.
pub(crate) async fn free_slots(shared: &Shared, now: Timestamp) -> Result<usize, StoreError> {
    let starts = shared.deps.admission.starts_since(hour_ago(now)).await?;
    let capacity = shared.settings.per_hour_runs as usize;
    Ok(capacity.saturating_sub(starts.len()))
}

/// When the oldest run still in the window frees its slot, if any run is.
pub(crate) async fn next_slot(shared: &Shared) -> Result<Option<Timestamp>, StoreError> {
    let now = shared.deps.clock.now();
    let starts = shared.deps.admission.starts_since(hour_ago(now)).await?;
    Ok(starts.first().and_then(|t| slot_after(*t)))
}
