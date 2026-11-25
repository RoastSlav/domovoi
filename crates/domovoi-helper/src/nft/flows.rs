//! Existing flows and the bridge gap: what a block does not reach, said plainly.
//!
//! The chain has no `ct` accept, so a non-offloaded established flow is cut by the drop. A
//! flowtable can still carry an offloaded flow, so when any flowtable exists the helper deletes
//! the source's conntrack entries and reports whether that worked. Interfaces enslaved to a
//! bridge never reach an `inet` prerouting hook the same way; the helper reports that gap.

use std::path::Path;

use domovoi_core::containment::{Family, Net, templates};

use super::Cx;

/// What happened to flows that were already established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFlows {
    /// A flowtable exists and the conntrack deletion exited 0.
    Cut,
    /// No flowtable exists, so the drop reaches every flow.
    NotApplicable,
    /// A flowtable exists and the deletion failed, is unsupported or was skipped as too wide.
    MayPersist,
}

impl ExistingFlows {
    /// The wire word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::NotApplicable => "n/a",
            Self::MayPersist => "may_persist",
        }
    }
}

/// Whether the listing of all flowtables holds one.
#[must_use]
pub fn has_flowtable(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("nftables").and_then(|n| n.as_array().cloned()))
        .is_some_and(|items| items.iter().any(|i| i.get("flowtable").is_some()))
}

fn narrow_enough(net: &Net) -> bool {
    match net.family() {
        Family::V4 => net.prefix_len() >= 24,
        Family::V6 => net.prefix_len() >= 120,
    }
}

/// Reports the state of existing flows for `net` after an apply, deleting conntrack entries
/// when flowtables exist and the target is a single host or a narrow prefix.
pub async fn existing_flows(cx: &Cx<'_>, net: &Net) -> ExistingFlows {
    let listed = cx.run.read(templates::list_flowtables_args()).await;
    let present = match &listed {
        Ok(run) if run.ok() => has_flowtable(&run.stdout),
        // A failed read cannot rule a flowtable out.
        _ => true,
    };
    if !present {
        return ExistingFlows::NotApplicable;
    }
    if !narrow_enough(net) {
        return ExistingFlows::MayPersist;
    }
    match cx.run.conntrack(templates::conntrack_delete_args(net)).await {
        Ok(run) if run.ok() => ExistingFlows::Cut,
        _ => ExistingFlows::MayPersist,
    }
}

/// Whether an interface with a `device` link (a physical NIC) has a `master` that is a bridge,
/// read from `sys_root` (`/sys` in production, a fixture tree in tests).
#[must_use]
pub fn bridge_gap(sys_root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(sys_root.join("class/net")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let dir = entry.path();
        dir.join("device").exists()
            && std::fs::canonicalize(dir.join("master"))
                .is_ok_and(|master| master.join("bridge").is_dir())
    })
}
