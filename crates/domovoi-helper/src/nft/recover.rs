//! Crash recovery: an `intent` with no result.
//!
//! The helper may die between the journal line and the `nft` call, or between the call and the
//! result line. The kernel's timeouts continue regardless. On the next request the sets are read
//! and each dangling intent is settled from membership: present means it happened, absent means
//! it did not.

use domovoi_core::containment::{JournalOp, JournalRecord, Net};
use jiff::SignedDuration;

use super::journal_file::{Journal, Loaded, Phase, fold};
use super::view::TableView;
use super::{Cx, Refusal};
use domovoi_core::containment::templates::{STEP1_TTL_S, SetName};

/// Settles every dangling intent against `view`. Returns whether the journal changed, so the
/// caller reads it again.
pub fn recover(cx: &Cx<'_>, view: &TableView, loaded: &Loaded) -> Result<bool, Refusal> {
    let now = cx.clock.now();
    let mut changed = false;
    for block in fold(&loaded.records) {
        let Phase::Intent(op) = block.phase else {
            if block.phase == Phase::RollbackIntent {
                changed |= settle_rollback(cx, &cx.journal, view, &block.source, &block.block)?;
            }
            continue;
        };
        let Ok(net) = Net::parse(&block.source) else {
            continue;
        };
        let set = SetName::for_net(&net).as_str();
        let member = view
            .sets
            .get(set)
            .and_then(|s| s.members.iter().find(|m| m.is(&net)));
        let record = match (op, member.and_then(|m| m.expires)) {
            (JournalOp::Rollback, _) => continue,
            (_, None) => JournalRecord::Failed {
                block: block.block.clone(),
                token: "crashed".to_owned(),
            },
            (_, Some(left)) if left > i64::from(STEP1_TTL_S) => JournalRecord::Confirmed {
                block: block.block.clone(),
                deadline: now + SignedDuration::from_secs(left),
            },
            (_, Some(left)) => JournalRecord::Applied {
                block: block.block.clone(),
                at: now - SignedDuration::from_secs(i64::from(STEP1_TTL_S) - left),
            },
        };
        cx.journal
            .append(cx.guard, &record)
            .map_err(Refusal::journal)?;
        changed = true;
    }
    Ok(changed)
}

fn settle_rollback(
    cx: &Cx<'_>,
    journal: &Journal,
    view: &TableView,
    source: &str,
    block: &str,
) -> Result<bool, Refusal> {
    let Ok(net) = Net::parse(source) else {
        return Ok(false);
    };
    let set = SetName::for_net(&net).as_str();
    let present = view
        .sets
        .get(set)
        .is_some_and(|s| s.members.iter().any(|m| m.is(&net)));
    if present {
        // The rollback may be retried by its caller; nothing is recorded as done.
        return Ok(false);
    }
    journal
        .append(
            cx.guard,
            &JournalRecord::RolledBack {
                block: block.to_owned(),
                at: cx.clock.now(),
            },
        )
        .map_err(Refusal::journal)?;
    Ok(true)
}
