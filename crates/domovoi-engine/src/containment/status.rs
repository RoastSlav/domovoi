//! What the helper said about one host's containment at the last owner-initiated refresh.
//!
//! The hub never polls for this. The value lives in memory only: after a restart a policy run is
//! refused (`drills_not_passed`) until the owner refreshes, which fails closed.

use domovoi_core::containment::{ContainOutputs, keys};
use jiff::Timestamp;
use serde_json::Value;

/// One element the helper reported in its block sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedBlock {
    /// The canonical network.
    pub source: String,
    /// The nft set that holds it.
    pub set: String,
    /// Seconds left on the element, when the helper said.
    pub remaining_s: Option<u64>,
    /// The helper's journal phase, when it has one.
    pub phase: Option<String>,
}

/// The helper's answer to `firewall.list_blocks`, as it was read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostContainment {
    /// When the hub read it.
    pub observed_at: Option<Timestamp>,
    /// The leak self-test gate: `open`, `blocked_leak` or `blocked_untested`.
    pub gate: String,
    /// The drills: `passed`, `failed` or `not_run`.
    pub drills: String,
    /// `ok`, `created` or `recreated`.
    pub table_state: Option<String>,
    /// `hosts` or `hosts_and_prefixes`.
    pub layout: Option<String>,
    /// Blocks lost with the table, as the helper listed them.
    pub lost_blocks: Option<String>,
    /// Whether a bridged physical interface leaves a gap.
    pub bridge_gap: Option<bool>,
    /// `cut`, `n/a` or `may_persist`.
    pub existing_flows: Option<String>,
    /// How many elements are active.
    pub active: Option<u64>,
    /// How many members have no journal entry.
    pub orphans: Option<u64>,
    /// The blocks the helper listed.
    pub blocks: Vec<ObservedBlock>,
}

impl HostContainment {
    /// Reads the outputs of a `firewall.list_blocks` answer received at `at`.
    #[must_use]
    pub fn from_outputs(at: Timestamp, outputs: &ContainOutputs) -> Self {
        let text = |key: &str| outputs.get(key).map(str::to_owned);
        let number = |key: &str| outputs.get(key).and_then(|v| v.parse::<u64>().ok());
        Self {
            observed_at: Some(at),
            gate: text(keys::OUT_GATE).unwrap_or_default(),
            drills: text(keys::OUT_DRILLS).unwrap_or_default(),
            table_state: text(keys::OUT_TABLE_STATE),
            layout: text(keys::OUT_LAYOUT),
            lost_blocks: text(keys::OUT_LOST_BLOCKS).filter(|v| !v.is_empty()),
            bridge_gap: outputs.get(keys::OUT_BRIDGE_GAP).map(|v| v == "true"),
            existing_flows: text(keys::OUT_EXISTING_FLOWS),
            active: number(keys::OUT_ACTIVE),
            orphans: number(keys::OUT_ORPHANS),
            blocks: outputs
                .get(keys::OUT_BLOCKS)
                .map(parse_blocks)
                .unwrap_or_default(),
        }
    }

    /// Whether the drills passed on this host's current kernel, `nft` and layout.
    #[must_use]
    pub fn drills_passed(&self) -> bool {
        self.drills.starts_with("passed")
    }
}

fn parse_blocks(text: &str) -> Vec<ObservedBlock> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(ObservedBlock {
                source: item.get("source")?.as_str()?.to_owned(),
                set: item
                    .get("set")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                remaining_s: item.get("remaining_s").and_then(Value::as_u64),
                phase: item.get("phase").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn outputs(pairs: &[(&str, &str)]) -> ContainOutputs {
        ContainOutputs(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn a_list_answer_is_read_into_the_host_status() {
        let status = HostContainment::from_outputs(
            Timestamp::UNIX_EPOCH,
            &outputs(&[
                (keys::OUT_GATE, "open"),
                (keys::OUT_DRILLS, "passed"),
                (keys::OUT_BRIDGE_GAP, "true"),
                (keys::OUT_ACTIVE, "1"),
                (
                    keys::OUT_BLOCKS,
                    r#"[{"source":"203.0.113.7/32","set":"blocked4","remaining_s":540,"phase":"confirmed"},{"nope":1}]"#,
                ),
            ]),
        );
        assert!(status.drills_passed());
        assert_eq!(status.bridge_gap, Some(true));
        assert_eq!(status.active, Some(1));
        assert_eq!(status.blocks.len(), 1);
        assert_eq!(status.blocks[0].remaining_s, Some(540));
    }

    #[test]
    fn a_missing_drills_answer_is_not_a_pass() {
        let status = HostContainment::from_outputs(Timestamp::UNIX_EPOCH, &outputs(&[]));
        assert!(!status.drills_passed());
        assert!(HostContainment::default().blocks.is_empty());
    }
}
