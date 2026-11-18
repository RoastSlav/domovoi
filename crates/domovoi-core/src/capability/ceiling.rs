//! The hub-side view of a host's unattended ceiling (04 §8.3, D-18).
//!
//! These are data types only: an agent summarises its `agent.toml [unattended]` section into a
//! [`CeilingSummary`], and the policy evaluator reads it. Nothing here parses a config file or
//! decides anything.
//!
//! A summary is data from an agent, so it is checked before it is trusted: [`validate`]
//! ([CeilingSummary::validate]) reads the numbers and the shape, and
//! [`validate_against`]([CeilingSummary::validate_against]) reads them against the contracts the
//! hub actually holds. A ceiling key the contracts do not declare, a zero rate or a bound outside
//! the contract's own schema is refused with a named problem, never quietly repaired. An entry is
//! kept even when `enabled` is false, so a owner's limits stay visible and a broken one is still
//! reported; turning the ceiling off is done by the flag, not by dropping entries.

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use serde::{Deserialize, Serialize};

use super::contract::CapabilityContract;
use super::effect::TargetKind;
use super::ids::{ContractId, TargetName};
use super::index::ContractIndex;
use super::schema::{declares_argument, stated_bounds};

/// The longest the reported configuration hash may be, in characters.
const MAX_HASH_CHARS: usize = 128;

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

/// One problem with a host's unattended ceiling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CeilingError {
    /// The rate is zero, which is not how a capability is forbidden: leaving its entry out is.
    #[error("{contract}: a rate of zero runs per hour forbids nothing and runs nothing")]
    ZeroRate {
        /// The entry.
        contract: ContractId,
    },
    /// No run may be active at once, which is the same contradiction as a zero rate.
    #[error("{contract}: zero active runs contradicts a rate above zero")]
    ZeroActive {
        /// The entry.
        contract: ContractId,
    },
    /// A TTL limit is zero or negative, so no run would ever be allowed to finish.
    #[error("{contract}: a TTL limit must be above zero")]
    NonPositiveTtl {
        /// The entry.
        contract: ContractId,
    },
    /// An entry names no targets, so it covers nothing and says so badly.
    #[error("{contract}: an entry naming targets must name at least one")]
    EmptyTargetNames {
        /// The entry.
        contract: ContractId,
    },
    /// The contract has no target to name, so an entry may only cover it as `Any`.
    #[error("{contract}: a contract with no named target may only be covered by `Any`")]
    NamesForUnnamedTarget {
        /// The entry.
        contract: ContractId,
    },
    /// The same contract has two entries, so the ceiling does not say which bound applies.
    #[error("{contract}: two entries cover the same contract")]
    DuplicateEntry {
        /// The contract.
        contract: ContractId,
    },
    /// The ceiling bounds a contract the hub does not have.
    #[error("{contract}: no such contract is known")]
    UnknownContract {
        /// The contract.
        contract: ContractId,
    },
    /// The ceiling bounds an argument the contract does not declare constrainable, so the
    /// bound would look authoritative and be ignored.
    #[error("{contract}: bounds `{key}`, which the contract does not declare constrainable")]
    UndeclaredArg {
        /// The contract.
        contract: ContractId,
        /// The argument.
        key: String,
    },
    /// The bound is outside what the contract's own schema states for that argument.
    #[error("{contract}: bound `{key}` is outside the contract's own limits")]
    BoundOutsideSchema {
        /// The contract.
        contract: ContractId,
        /// The argument.
        key: String,
    },
    /// The reported configuration hash is blank, not hexadecimal or too long.
    #[error("the configuration hash must be 1 to 128 lowercase hexadecimal characters")]
    MalformedHash,
}

impl CeilingSummary {
    /// Checks the summary's own numbers and shape: no zero rates, no empty target sets, no
    /// duplicate contract, and a hash in the canonical form the agent reports.
    ///
    /// Entries are checked whether or not `enabled` is set, because a disabled ceiling still
    /// carries the owner's limits and a broken one has to be reported, not discarded.
    pub fn validate(&self) -> Result<(), CeilingError> {
        if let Some(hash) = &self.hash
            && !valid_hash(hash)
        {
            return Err(CeilingError::MalformedHash);
        }
        let mut seen = BTreeSet::<ContractId>::new();
        for entry in &self.entries {
            check_entry_numbers(entry)?;
            if !seen.insert(entry.contract.clone()) {
                return Err(CeilingError::DuplicateEntry {
                    contract: entry.contract.clone(),
                });
            }
        }
        Ok(())
    }

    /// Checks the summary against the contracts the hub holds, so every key and bound means
    /// something the evaluator can read.
    ///
    /// A contract the hub does not have is refused, so a ceiling can never look like it permits a
    /// capability the model cannot name. An argument is only bounded when the contract declares
    /// it constrainable, and the bound has to sit inside the limits the contract's own schema
    /// states for it.
    pub fn validate_against(&self, index: &ContractIndex) -> Result<(), CeilingError> {
        self.validate()?;
        for entry in &self.entries {
            let Some(contract) = index.get(&entry.contract) else {
                return Err(CeilingError::UnknownContract {
                    contract: entry.contract.clone(),
                });
            };
            check_entry_targets(contract, entry)?;
            check_entry_args(contract, entry)?;
        }
        Ok(())
    }
}

/// Whether the reported hash is 1 to 128 lowercase hexadecimal characters.
fn valid_hash(hash: &str) -> bool {
    !hash.is_empty()
        && hash.len() <= MAX_HASH_CHARS
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Refuses an entry whose own numbers or targets contradict each other.
///
/// A rate or an active count of zero forbids nothing and runs nothing: the way to forbid a
/// capability is to leave its entry out. A TTL limit must be above zero, and an entry that names
/// targets has to name at least one.
fn check_entry_numbers(entry: &CeilingEntry) -> Result<(), CeilingError> {
    if entry.max_per_hour == 0 {
        return Err(CeilingError::ZeroRate {
            contract: entry.contract.clone(),
        });
    }
    if matches!(&entry.max_active, Some(0)) {
        return Err(CeilingError::ZeroActive {
            contract: entry.contract.clone(),
        });
    }
    if matches!(&entry.max_ttl, Some(ttl) if *ttl <= SignedDuration::ZERO) {
        return Err(CeilingError::NonPositiveTtl {
            contract: entry.contract.clone(),
        });
    }
    if matches!(&entry.targets, CeilingTargets::Names(names) if names.is_empty()) {
        return Err(CeilingError::EmptyTargetNames {
            contract: entry.contract.clone(),
        });
    }
    Ok(())
}

/// Refuses an entry whose targets do not fit the contract's target kind.
///
/// `None` names no target at all and `Host` is the host itself, so neither has a target an entry
/// may name: only `Any` covers them. `Container` and `Unit` name one, so a set of names is
/// allowed and must not be empty.
fn check_entry_targets(
    contract: &CapabilityContract,
    entry: &CeilingEntry,
) -> Result<(), CeilingError> {
    match contract.target_kind {
        TargetKind::Container | TargetKind::Unit => Ok(()),
        TargetKind::None | TargetKind::Host if matches!(&entry.targets, CeilingTargets::Names(names) if !names.is_empty()) => {
            Err(CeilingError::NamesForUnnamedTarget {
                contract: entry.contract.clone(),
            })
        }
        TargetKind::None | TargetKind::Host => Ok(()),
    }
}

/// Refuses a bound the contract does not declare constrainable, and one outside the limits the
/// contract's own schema states for that argument.
fn check_entry_args(
    contract: &CapabilityContract,
    entry: &CeilingEntry,
) -> Result<(), CeilingError> {
    let schema = &contract.input_schema;
    for (key, value) in entry.args.iter() {
        let declared = contract
            .constrainable_args
            .iter()
            .any(|arg| arg.key == *key);
        if !declared || !declares_argument(schema, key) {
            return Err(CeilingError::UndeclaredArg {
                contract: entry.contract.clone(),
                key: key.to_owned(),
            });
        }
        let (low, high) = stated_bounds(schema, key);
        let outside = match (low, high) {
            (Some(low), _) if *value < low => true,
            (_, Some(high)) if *value > high => true,
            _ => false,
        };
        if outside {
            return Err(CeilingError::BoundOutsideSchema {
                contract: entry.contract.clone(),
                key: key.to_owned(),
            });
        }
    }
    Ok(())
}
