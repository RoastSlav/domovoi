//! The hub-side view of a host's unattended ceiling (04 §8.3, D-18).
//!
//! These are data types only: an agent summarises its `agent.toml [unattended]` section into
//! a [`CeilingSummary`], and the policy evaluator reads it. Nothing here parses a config file
//! or decides anything.

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use serde::{Deserialize, Serialize};

use super::ids::{ContractId, TargetName};

/// The part of a host's unattended ceiling the hub sees.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CeilingSummary {
    /// `[unattended].enabled`.
    pub enabled: bool,
    /// One entry per contract the ceiling covers.
    #[serde(default)]
    pub entries: Vec<CeilingEntry>,
    /// Opaque hash of the agent's ceiling configuration, as reported. The hub compares it
    /// for change detection and never interprets it.
    #[serde(default)]
    pub hash: Option<String>,
}

/// One contract's bound in a host's unattended ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeilingEntry {
    /// The exact contract id; a ceiling names no wildcards.
    pub contract: ContractId,
    /// Which targets the entry covers.
    pub targets: CeilingTargets,
    /// The most runs allowed in any hour.
    pub max_per_hour: u32,
    /// How many runs may be active at once; a hub default may be stricter only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active: Option<u32>,
    /// The longest TTL allowed; a hub default may be stricter only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "jiff::fmt::serde::duration::friendly::compact::optional"
    )]
    pub max_ttl: Option<SignedDuration>,
    /// Bounds on the contract's declared constrainable args: key to integer bound. The
    /// contract's `ConstraintKind` says whether the bound is a minimum or a maximum.
    #[serde(default)]
    pub args: BTreeMap<String, i64>,
}

/// Which targets a ceiling entry covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "names", rename_all = "snake_case")]
pub enum CeilingTargets {
    /// Every target of the contract.
    Any,
    /// Only the named targets.
    Names(BTreeSet<TargetName>),
}
