//! The one-shot retirement of legacy actions.
//!
//! The first-generation executor is gone, so an action that was still open when the hub
//! was upgraded can never run, be approved or be verified. [`drain_legacy_actions`] settles
//! each one honestly at the first start, once, with an audit row and a journal row, and
//! leaves every row readable. Nothing is converted or deleted.
//!
//! Every move is one transaction guarded by the status the row was read in, so a row someone
//! else moved meanwhile is skipped rather than overwritten.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionFilter, ActionGuard, ActionRepo, AggregateKind, AggregateRef, AuditDraft, Clock,
    CommitError, EffectCancel, EffectQueue, EffectRead, EffectStatus, IncidentRepo, JournalDraft,
    JournalKind, Page, StateWrite, StoreError, Transaction, UnitOfWork,
};
use domovoi_core::{ActionRequest, Actor};
use jiff::Timestamp;
use serde_json::{Value, json};

/// Why an open proposal or approval was settled at the upgrade.
const EXPIRED_REASON: &str = "legacy action expired at upgrade";
/// Why a run that was handed to an agent is unresolved.
const UNKNOWN_REASON: &str = "legacy executor removed before this run was reconciled";
/// Why a pending verification was dropped.
const VERIFICATION_REASON: &str = "legacy verification dropped at upgrade";

/// Audit kind for a proposal expired at the upgrade.
const EXPIRED: &str = "action.expired";
/// Audit kind for an approval that will never run.
const NOT_RUN: &str = "action.not_run";
/// Audit kind for a run whose result cannot be established.
const OUTCOME_UNKNOWN: &str = "action.outcome_unknown";
/// Audit kind for a verification that was dropped.
pub const VERIFICATION_DROPPED: &str = "action.verification_dropped";

/// The component name the retirement acts as; it displays as `system:legacy`.
const ACTOR: &str = "legacy";

/// How many tries a listing gets before the run gives up.
const SCAN_ATTEMPTS: usize = 3;

/// The ports the retirement reads and writes through.
pub struct LegacyDrainDeps {
    /// Where actions are listed.
    pub actions: Arc<dyn ActionRepo>,
    /// Incidents; kept so the retirement can name them in later reports.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Commits each move with its audit and journal rows.
    pub uow: Arc<dyn UnitOfWork>,
    /// Returns effects claimed by the previous session to pending.
    pub effects: Arc<dyn EffectQueue>,
    /// Reads the effect outbox.
    pub effect_read: Arc<dyn EffectRead>,
    /// The time source.
    pub clock: Arc<dyn Clock>,
}

/// What one run of the retirement did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegacyDrainReport {
    /// Proposals moved to `Expired`.
    pub expired: u32,
    /// Approvals moved to `NotRun`.
    pub not_run: u32,
    /// Dispatched runs moved to `OutcomeUnknown`.
    pub outcome_unknown: u32,
    /// Succeeded actions whose pending verification was dropped.
    pub verification_dropped: u32,
    /// Effects cancelled in total.
    pub effects_cancelled: u64,
}

impl LegacyDrainReport {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Why the retirement could not finish.
#[derive(Debug, thiserror::Error)]
pub enum LegacyDrainError {
    /// The store failed; the next start repeats the run.
    #[error("store error: {0}")]
    Store(String),
}

impl From<StoreError> for LegacyDrainError {
    fn from(err: StoreError) -> Self {
        Self::Store(err.to_string())
    }
}

/// Retires every open legacy action, once, and returns what it did. Idempotent: a second
/// run finds nothing.
///
/// # Errors
/// [`LegacyDrainError::Store`] when the store will not list the actions or a commit fails
/// for a reason other than the row having moved.
pub async fn drain_legacy_actions(
    deps: &LegacyDrainDeps,
) -> Result<LegacyDrainReport, LegacyDrainError> {
    let mut report = LegacyDrainReport::default();
    deps.effects
        .revert_claimed_at_start(&[AggregateKind::Action])
        .await?;
    let pending = pending_by_action(deps).await?;
    for status in ["proposed", "approved", "dispatched"] {
        for action in scan(deps, status).await? {
            let now = deps.clock.now();
            let id = action.id().to_string();
            if retire(deps, action, status, now).await? {
                report.effects_cancelled += pending.get(&id).copied().unwrap_or(0);
                match status {
                    "proposed" => report.expired += 1,
                    "approved" => report.not_run += 1,
                    _ => report.outcome_unknown += 1,
                }
            }
        }
    }
    drop_verifications(deps, &pending, &mut report).await?;
    if !report.is_empty() {
        tracing::info!(
            expired = report.expired,
            not_run = report.not_run,
            outcome_unknown = report.outcome_unknown,
            verification_dropped = report.verification_dropped,
            effects_cancelled = report.effects_cancelled,
            "retired the open legacy actions",
        );
    }
    Ok(report)
}

/// Every action in `status`, paged to the end and read again from the start when a page
/// fails, so a store hiccup is never read as a listing that finished.
async fn scan(
    deps: &LegacyDrainDeps,
    status: &str,
) -> Result<Vec<ActionRequest>, LegacyDrainError> {
    let filter = ActionFilter {
        status: Some(status.to_owned()),
        ..ActionFilter::default()
    };
    let mut attempt = 1;
    let mut found = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        match deps.actions.list(&filter, &page).await {
            Ok(got) => {
                found.extend(got.items);
                match got.next_cursor {
                    Some(cursor) => page.cursor = Some(cursor),
                    None => return Ok(found),
                }
            }
            Err(err) if attempt < SCAN_ATTEMPTS => {
                tracing::warn!(%err, attempt, "the action listing failed; starting over");
                attempt += 1;
                found.clear();
                page = Page::first(Page::MAX_LIMIT);
            }
            Err(err) => return Err(err.into()),
        }
    }
}

/// Moves one action out of `from` in one transaction. `false` when someone else moved the row.
async fn retire(
    deps: &LegacyDrainDeps,
    mut action: ActionRequest,
    from: &str,
    now: Timestamp,
) -> Result<bool, LegacyDrainError> {
    let (kind, reason) = match from {
        "proposed" => {
            if action.expire_at_upgrade(now).is_err() {
                return Ok(false);
            }
            (EXPIRED, EXPIRED_REASON)
        }
        "approved" => {
            if action.never_ran(EXPIRED_REASON.to_owned(), now).is_err() {
                return Ok(false);
            }
            (NOT_RUN, EXPIRED_REASON)
        }
        _ => {
            if action
                .outcome_unknown(UNKNOWN_REASON.to_owned(), now)
                .is_err()
            {
                return Ok(false);
            }
            (OUTCOME_UNKNOWN, UNKNOWN_REASON)
        }
    };
    action.clear_chat_token_hash();
    let audit = event(kind, &action, now, reason);
    let journal = journal_for(&action, from, &audit, now, reason);
    let tx = Transaction::new()
        .write(StateWrite::Action {
            action: action.clone(),
            expected: ActionGuard::Status(from.to_owned()),
        })
        .audit(audit)
        .journal(journal)
        .cancel_effects(EffectCancel {
            aggregate: AggregateRef::action(action.id()),
            at: now,
        });
    match deps.uow.commit(tx).await {
        Ok(_) => Ok(true),
        Err(CommitError::Conflict { .. }) => Ok(false),
        Err(err) => Err(LegacyDrainError::Store(err.to_string())),
    }
}

/// How many pending effects each action has, keyed by the action id.
async fn pending_by_action(
    deps: &LegacyDrainDeps,
) -> Result<BTreeMap<String, u64>, LegacyDrainError> {
    let mut counts = BTreeMap::new();
    for effect in deps
        .effect_read
        .effects_with_status(EffectStatus::Pending, 500)
        .await?
    {
        if effect.aggregate.kind == AggregateKind::Action {
            *counts.entry(effect.aggregate.id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

/// Cancels the pending verification wake of every succeeded action and records that it did.
async fn drop_verifications(
    deps: &LegacyDrainDeps,
    counts: &BTreeMap<String, u64>,
    report: &mut LegacyDrainReport,
) -> Result<(), LegacyDrainError> {
    let pending = deps
        .effect_read
        .effects_with_status(EffectStatus::Pending, 500)
        .await?;
    let mut seen = std::collections::BTreeSet::new();
    for effect in pending {
        if effect.kind != "wake" || effect.aggregate.kind != AggregateKind::Action {
            continue;
        }
        if effect.payload.get("purpose").and_then(Value::as_str) != Some("verify_action") {
            continue;
        }
        if !seen.insert(effect.aggregate.id.clone()) {
            continue;
        }
        let Ok(id) = effect.aggregate.id.parse() else {
            continue;
        };
        let Some(action) = deps.actions.get(id).await? else {
            continue;
        };
        if action.status().name() != "succeeded" {
            continue;
        }
        let now = deps.clock.now();
        let audit = event(VERIFICATION_DROPPED, &action, now, VERIFICATION_REASON);
        let tx = Transaction::new()
            .audit(audit)
            .cancel_effects(EffectCancel {
                aggregate: AggregateRef::action(action.id()),
                at: now,
            });
        match deps.uow.commit(tx).await {
            Ok(_) => {
                report.verification_dropped += 1;
                report.effects_cancelled += counts.get(&effect.aggregate.id).copied().unwrap_or(0);
            }
            Err(CommitError::Conflict { .. }) => {}
            Err(err) => return Err(LegacyDrainError::Store(err.to_string())),
        }
    }
    Ok(())
}

/// The audit row for one retirement: the same fields the action writer recorded for the
/// same kinds, plus the reason.
fn event(kind: &str, action: &ActionRequest, at: Timestamp, reason: &str) -> AuditDraft {
    let detail = json!({
        "incident": action.incident().to_string(),
        "skill": action.skill().as_str(),
        "host": action.host().to_string(),
        "plan_hash": action.plan_hash().short(),
        "status": action.status(),
        "reason": reason,
    });
    AuditDraft::new(
        Actor::System(ACTOR.to_owned()),
        at,
        kind.to_owned(),
        format!("action:{}", action.id()),
        detail,
    )
}

/// The journal row one retirement writes, in the shape the action writer used.
fn journal_for(
    action: &ActionRequest,
    from: &str,
    audit: &AuditDraft,
    now: Timestamp,
    reason: &str,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::action(action.id()),
        kind: JournalKind::ActionTransition,
        payload_v: 1,
        payload: json!({
            "action": action.id().to_string(),
            "incident": action.incident().to_string(),
            "host": action.host().to_string(),
            "skill": action.skill().as_str(),
            "from": from,
            "to": action.status().name(),
            "plan_hash": action.plan_hash().to_string(),
            "reason": reason,
        }),
        actor: audit.actor.clone(),
        occurred_at: now,
    }
}
