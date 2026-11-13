//! The cross-table invariants F1 guarantees.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::ports::{
    ActionFilter, AggregateRef, AuditFilter, IncidentFilter, JournalKind, JournalRead, Page,
    StoreError,
};
use domovoi_core::{ActionId, IncidentId};
use serde_json::Value;

use super::SweepStore;

/// The audit kinds that correspond one-to-one with a non-rotation action transition.
const TRANSITION_AUDITS: [&str; 11] = [
    "action.proposed",
    "action.approved",
    "action.denied",
    "action.expired",
    "action.rejected",
    "action.blocked",
    "action.dispatched",
    "action.succeeded",
    "action.failed",
    "action.verified",
    "action.verify_failed",
];

/// The audit kind for a chat token rotation.
const ROTATION_AUDIT: &str = "action.token_rotated";

/// Rows that predate the journal, so the invariants cannot expect their journal rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exempt {
    /// Actions inserted outside the writer that have no journal rows at all.
    pub actions: BTreeSet<ActionId>,
    /// How many timeline entries per incident predate the journal.
    pub legacy_timeline: BTreeMap<IncidentId, usize>,
}

impl Exempt {
    /// Nothing is exempt.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// Checks the cross-table invariants of F1. Returns every violation found, not just the
/// first.
pub async fn check_invariants<S: SweepStore>(store: &S) -> Result<(), Vec<String>> {
    check_invariants_except(store, &Exempt::none()).await
}

/// Checks the F1 invariants, allowing the rows named in `exempt` to predate the journal.
pub async fn check_invariants_except<S: SweepStore>(
    store: &S,
    exempt: &Exempt,
) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();

    let actions = match all_actions(store).await {
        Ok(actions) => actions,
        Err(err) => return Err(vec![format!("I1: the actions could not be read: {err}")]),
    };

    for action in &actions {
        let aggregate = AggregateRef::action(action.id());
        let entries = match JournalRead::entries_for(store, &aggregate, Page::MAX_LIMIT).await {
            Ok(entries) => entries,
            Err(err) => {
                violations.push(format!(
                    "I1: the journal for action {} could not be read: {err}",
                    action.id()
                ));
                continue;
            }
        };
        let transitions: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry.kind == JournalKind::ActionTransition.as_str() && !is_rotation(&entry.payload)
            })
            .collect();
        let rotations = entries
            .iter()
            .filter(|entry| {
                entry.kind == JournalKind::ActionTransition.as_str() && is_rotation(&entry.payload)
            })
            .count();

        if transitions.is_empty() {
            let exempt_action = entries.is_empty() && exempt.actions.contains(&action.id());
            if !exempt_action {
                violations.push(format!(
                    "I1: action {} has status {} but no transition journal row",
                    action.id(),
                    action.status().name()
                ));
            }
            continue;
        }

        let last_to = transitions
            .last()
            .and_then(|entry| entry.payload.get("to"))
            .and_then(Value::as_str);
        if last_to != Some(action.status().name()) {
            violations.push(format!(
                "I1: action {} has status {} but its last journal row says {}",
                action.id(),
                action.status().name(),
                last_to.unwrap_or("<missing>")
            ));
        }

        let audit_kinds = match audit_kinds_for(store, action.id()).await {
            Ok(kinds) => kinds,
            Err(err) => {
                violations.push(format!(
                    "I2: the audit for action {} could not be read: {err}",
                    action.id()
                ));
                continue;
            }
        };
        let transition_audits = audit_kinds
            .iter()
            .filter(|kind| TRANSITION_AUDITS.contains(&kind.as_str()))
            .count();
        if transition_audits != transitions.len() {
            violations.push(format!(
                "I2: action {} has {} transition journal rows but {transition_audits} transition audit rows",
                action.id(),
                transitions.len()
            ));
        }
        let rotation_audits = audit_kinds
            .iter()
            .filter(|kind| kind.as_str() == ROTATION_AUDIT)
            .count();
        if rotation_audits != rotations {
            violations.push(format!(
                "I2: action {} has {rotations} rotation journal rows but {rotation_audits} rotation audit rows",
                action.id()
            ));
        }
    }

    let incidents = match all_incidents(store).await {
        Ok(incidents) => incidents,
        Err(err) => return Err(vec![format!("I3: the incidents could not be read: {err}")]),
    };
    for incident in &incidents {
        let timeline = match domovoi_core::ports::IncidentRepo::timeline(store, incident.id()).await
        {
            Ok(timeline) => timeline,
            Err(err) => {
                violations.push(format!(
                    "I3: the timeline for incident {} could not be read: {err}",
                    incident.id()
                ));
                continue;
            }
        };
        let entries = match JournalRead::entries_for(
            store,
            &AggregateRef::incident(incident.id()),
            Page::MAX_LIMIT,
        )
        .await
        {
            Ok(entries) => entries,
            Err(err) => {
                violations.push(format!(
                    "I3: the journal for incident {} could not be read: {err}",
                    incident.id()
                ));
                continue;
            }
        };
        let written = entries
            .iter()
            .filter(|entry| entry.kind == JournalKind::IncidentTimeline.as_str())
            .count();
        let legacy = exempt
            .legacy_timeline
            .get(&incident.id())
            .copied()
            .unwrap_or(0);
        let expected = timeline.len().saturating_sub(legacy);
        if expected != written {
            violations.push(format!(
                "I3: incident {} has {expected} journaled timeline entries but {written} journal rows",
                incident.id()
            ));
        }
    }

    // Orphan timeline entries and signal links cannot be enumerated through the ports:
    // both are only readable per existing incident. Actions may outlive discarded
    // incidents, and the brief exempts them, so there is nothing left to check here.

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn is_rotation(payload: &Value) -> bool {
    payload.get("change").and_then(Value::as_str) == Some("chat_token_rotated")
}

/// Every action, following cursor pages.
async fn all_actions<S: SweepStore>(
    store: &S,
) -> Result<Vec<domovoi_core::ActionRequest>, StoreError> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got =
            domovoi_core::ports::ActionRepo::list(store, &ActionFilter::default(), &page).await?;
        items.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}

/// Every incident, following cursor pages.
async fn all_incidents<S: SweepStore>(
    store: &S,
) -> Result<Vec<domovoi_core::Incident>, StoreError> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = domovoi_core::ports::IncidentRepo::list(store, &IncidentFilter::default(), &page)
            .await?;
        items.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}

/// Every audit kind recorded for `action`.
async fn audit_kinds_for<S: SweepStore>(
    store: &S,
    action: ActionId,
) -> Result<Vec<String>, StoreError> {
    let filter = AuditFilter {
        subject: Some(format!("action:{action}")),
        ..AuditFilter::default()
    };
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = domovoi_core::ports::AuditLog::list(store, &filter, &page).await?;
        items.extend(got.items.into_iter().map(|event| event.kind));
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}
