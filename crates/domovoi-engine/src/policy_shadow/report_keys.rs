//! Stable keys for publication findings, locked-rule differences and the preview hash that
//! binds a publication to exactly what the owner reviewed.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::policy::{Finding, FindingKind, Rule, RuleId};

/// What happened to a locked rule between two rule sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockChangeKind {
    /// A rule is locked in the new set and was not locked (or absent) in the base.
    Created,
    /// A rule is locked in both and differs in any field.
    Edited,
    /// A rule was locked in the base and is present but not locked now.
    Unlocked,
    /// A rule was locked in the base and is gone.
    Removed,
}

impl LockChangeKind {
    /// The snake_case name used in audit rows and hashes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Edited => "edited",
            Self::Unlocked => "unlocked",
            Self::Removed => "removed",
        }
    }
}

/// One locked-rule difference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LockChange {
    /// The rule.
    pub rule: RuleId,
    /// What happened to it.
    pub change: LockChangeKind,
}

/// Locked-rule differences between `base` and `next`, sorted by rule id. Disabling, editing
/// or deleting a lock is a change; an unrelated unlocked edit is not.
#[must_use]
pub fn locked_changes(base: &[Rule], next: &[Rule]) -> Vec<LockChange> {
    let base_by_id: BTreeMap<&RuleId, &Rule> = base.iter().map(|r| (&r.id, r)).collect();
    let next_by_id: BTreeMap<&RuleId, &Rule> = next.iter().map(|r| (&r.id, r)).collect();
    let mut changes = Vec::new();
    for (id, new) in &next_by_id {
        match base_by_id.get(id) {
            _ if !new.locked => {}
            Some(old) if old.locked => {
                if old != new {
                    changes.push(change(id, LockChangeKind::Edited));
                }
            }
            _ => changes.push(change(id, LockChangeKind::Created)),
        }
    }
    for (id, old) in &base_by_id {
        if !old.locked {
            continue;
        }
        match next_by_id.get(id) {
            None => changes.push(change(id, LockChangeKind::Removed)),
            Some(new) if !new.locked => changes.push(change(id, LockChangeKind::Unlocked)),
            Some(_) => {}
        }
    }
    changes.sort();
    changes
}

fn change(id: &RuleId, kind: LockChangeKind) -> LockChange {
    LockChange {
        rule: (*id).clone(),
        change: kind,
    }
}

/// `"<kind>:<first>:<second>"`, the kind in snake_case.
#[must_use]
pub fn finding_key(finding: &Finding) -> String {
    let kind = match finding.kind {
        FindingKind::EqualScopeConflict => "equal_scope_conflict",
        FindingKind::IncomparableConflict => "incomparable_conflict",
        FindingKind::IneffectiveUnderLock => "ineffective_under_lock",
        FindingKind::TargetOutsideEveryCeiling => "target_outside_every_ceiling",
    };
    format!("{kind}:{}:{}", finding.first, finding.second)
}

/// Lowercase hex SHA-256 over the base version number, the rule-set hash, the sorted
/// blocking keys and the sorted locked changes. A hash from a different rule set, base or
/// finding set never matches.
#[must_use]
pub fn preview_hash(
    base_number: u32,
    rules_hash: &str,
    blocking: &[String],
    locked: &[LockChange],
) -> String {
    let mut keys: Vec<&String> = blocking.iter().collect();
    keys.sort();
    let mut changes: Vec<(&str, &str)> = locked
        .iter()
        .map(|c| (c.rule.as_str(), c.change.as_str()))
        .collect();
    changes.sort_unstable();
    let doc = serde_json::json!({
        "v": 1,
        "base": base_number,
        "rules": rules_hash,
        "blocking": keys,
        "locked": changes,
    });
    hex::encode(Sha256::digest(doc.to_string().as_bytes()))
}
