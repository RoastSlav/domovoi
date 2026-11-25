//! The leak self-test result, the gate it maps to, and the drill state.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::templates::Layout;

/// Why containment is blocked as untested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UntestedReason {
    /// No result is stored.
    NeverRun,
    /// A scenario was inconclusive.
    Inconclusive,
    /// The kernel, `nft` version or layout changed since the result.
    KeyChanged,
    /// An untested platform without the owner's per-host opt-in.
    PlatformUntested,
}

/// Whether containment writes may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Gate {
    /// Open; `interval_path` says prefixes may be blocked too.
    Open {
        /// Whether the interval layout passed.
        interval_path: bool,
    },
    /// The plain layout leaked: everything is refused.
    BlockedLeak,
    /// No usable passing result.
    BlockedUntested {
        /// Why.
        reason: UntestedReason,
    },
}

/// What a self-test result is valid for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelftestKey {
    /// `uname -r`.
    pub kernel_release: String,
    /// The `nft --version` text.
    pub nft_version: String,
    /// The layout tested.
    pub layout: Layout,
}

/// The verdict of one set type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// No leaked packet, the control leaked, the replacement minimum was reached.
    Pass,
    /// At least one leaked packet.
    Fail {
        /// How many.
        leaked: u64,
    },
    /// The test could not see a window or could not finish.
    Inconclusive {
        /// Why, as a short token such as `replacements_not_reached`.
        reason: String,
    },
}

/// The stored result of `nft-selftest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelftestResult {
    /// What it is valid for.
    pub key: SelftestKey,
    /// The plain-set verdict.
    pub hosts: Verdict,
    /// The interval-set verdict, when that layout was tested.
    pub prefixes: Option<Verdict>,
    /// When it ran.
    pub ran_at: Timestamp,
    /// Replacements achieved per scenario, the lowest.
    pub replacements: u64,
    /// The share of packets the split control leaked.
    pub control_leak_ratio: f64,
}

/// The gate for a stored result against the current key.
///
/// `write_allowed` is the platform tier decision: false on an untested platform without the
/// owner's opt-in. A failure to run, a missing result or a different key never opens it.
#[must_use]
pub fn gate_for(
    result: Option<&SelftestResult>,
    current: &SelftestKey,
    write_allowed: bool,
) -> Gate {
    let blocked = |reason| Gate::BlockedUntested { reason };
    if !write_allowed {
        return blocked(UntestedReason::PlatformUntested);
    }
    let Some(result) = result else {
        return blocked(UntestedReason::NeverRun);
    };
    if result.key != *current {
        return blocked(UntestedReason::KeyChanged);
    }
    if matches!(result.hosts, Verdict::Fail { .. }) {
        return Gate::BlockedLeak;
    }
    let inconclusive = matches!(result.hosts, Verdict::Inconclusive { .. })
        || matches!(result.prefixes, Some(Verdict::Inconclusive { .. }));
    if inconclusive {
        return blocked(UntestedReason::Inconclusive);
    }
    Gate::Open {
        interval_path: matches!(result.prefixes, Some(Verdict::Pass)),
    }
}

/// Whether the drill suite passed on this host with the current key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DrillsState {
    /// Never run, or run under a different key.
    NotRun,
    /// All drills passed.
    Passed {
        /// When.
        at: Timestamp,
    },
    /// A drill failed.
    Failed {
        /// When.
        at: Timestamp,
    },
}
