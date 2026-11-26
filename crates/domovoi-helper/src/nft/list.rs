//! `firewall.list_blocks`: the table, the gate, the drills and every block, reconciled.
//!
//! Membership is read from the sets as JSON. A member with no journal entry is an orphan: it is
//! reported, still bounded by the set's default timeout, and counted toward `max_active`.

use domovoi_core::containment::templates::SetName;
use domovoi_core::containment::{JournalRecord, Layout, Net, keys, templates};
use serde_json::json;

use super::flows::{bridge_gap, has_flowtable};
use super::journal_file::{Phase, fold};
use super::limits::{active_count, counted_sets};
use super::{Cx, OUT_FLOWTABLES, Outputs, Reply, State};

/// The most bytes of the `blocks` text; a larger list is cut and says so.
const BLOCKS_CAP: usize = 200_000;

fn phase_word(phase: Phase) -> &'static str {
    match phase {
        Phase::Intent(_) => "intent",
        Phase::Applied => "applied",
        Phase::Confirmed => "confirmed",
        Phase::RollbackIntent => "rollback_intent",
        Phase::RolledBack => "rolled_back",
        Phase::Lost => "lost",
        Phase::Failed => "failed",
        Phase::Noop => "noop",
    }
}

/// Serves `firewall.list_blocks`.
pub async fn list(cx: &Cx<'_>, st: &State) -> Reply {
    let now = cx.clock.now();
    let interval_path = st.ens.layout == Layout::HostsAndPrefixes;
    let blocks = fold(&st.loaded.records);
    let mut rows = Vec::new();
    let mut orphans = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let parsed: Vec<(&super::journal_file::BlockState, Net)> = blocks
        .iter()
        .filter_map(|b| Net::parse(&b.source).ok().map(|n| (b, n)))
        .collect();
    for (set, member) in st.ens.view.members_of(counted_sets(interval_path)) {
        let owner = parsed
            .iter()
            .find(|(_, net)| member.is(net) && SetName::for_net(net).as_str() == set)
            .map(|(block, _)| *block);
        match owner {
            Some(block) => {
                seen.insert(block.block.clone());
                rows.push(json!({
                    "block": block.block,
                    "source": block.source,
                    "set": set,
                    "remaining_s": member.expires,
                    "phase": phase_word(block.phase),
                }));
            }
            None => orphans.push(member.text()),
        }
    }
    for block in blocks
        .iter()
        .filter(|b| b.live(now) && !seen.contains(&b.block))
    {
        rows.push(json!({
            "block": block.block,
            "source": block.source,
            "set": block.set,
            "remaining_s": 0,
            "phase": "absent",
        }));
    }
    for element in &orphans {
        let recorded = st
            .loaded
            .records
            .iter()
            .any(|r| matches!(r, JournalRecord::Orphan { element: e } if e == element));
        if !recorded {
            let _ = cx.journal.append(
                cx.guard,
                &JournalRecord::Orphan {
                    element: element.clone(),
                },
            );
        }
    }
    let mut text = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_owned());
    while text.len() > BLOCKS_CAP && !rows.is_empty() {
        rows.truncate(rows.len() / 2);
        text = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_owned());
    }
    let flowtables = match cx.run.read(templates::list_flowtables_args()).await {
        Ok(run) if run.ok() => usize::from(has_flowtable(&run.stdout)),
        _ => 0,
    };
    let mut out: Outputs = Outputs::new();
    out.insert(
        keys::OUT_TABLE_STATE.to_owned(),
        st.ens.state.as_str().to_owned(),
    );
    out.insert(
        keys::OUT_LAYOUT.to_owned(),
        st.ens.layout.as_str().to_owned(),
    );
    out.insert(keys::OUT_GATE.to_owned(), st.facts.key_text.clone());
    out.insert(keys::OUT_DRILLS.to_owned(), st.facts.drills.clone());
    out.insert(keys::OUT_BLOCKS.to_owned(), text);
    out.insert(
        keys::OUT_ACTIVE.to_owned(),
        active_count(&st.ens.view, interval_path).to_string(),
    );
    out.insert(
        keys::OUT_ORPHANS.to_owned(),
        serde_json::to_string(&orphans).unwrap_or_else(|_| "[]".to_owned()),
    );
    out.insert(OUT_FLOWTABLES.to_owned(), flowtables.to_string());
    out.insert(
        keys::OUT_BRIDGE_GAP.to_owned(),
        bridge_gap(cx.sys_root).to_string(),
    );
    if !st.ens.lost.is_empty() {
        out.insert(keys::OUT_LOST_BLOCKS.to_owned(), st.ens.lost.join(","));
    }
    if let Some(drift) = &st.ens.drift {
        out.insert("table_drift".to_owned(), drift.clone());
    }
    Ok(out)
}
