//! The unattended ceiling: what a `policy`-labelled request may do on this host.
//!
//! Built from `agent.toml [unattended]`, local and root-owned. The hub cannot widen it: the
//! agent checks every policy-labelled request against it, and tells the hub only a summary and
//! its hash. With no section there is no ceiling and no policy-labelled request is accepted.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_config::UnattendedSection;
use domovoi_core::capability::{
    CeilingEntry, CeilingSummary, CeilingTargets, ContractId, TargetName,
};
use domovoi_proto::Sha256Bytes;
use jiff::SignedDuration;
use sha2::{Digest, Sha256};

/// The step of the ordered ceiling check that refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeilingStep {
    /// `[unattended].enabled` is false.
    Disabled,
    /// No entry names the exact contract.
    NoEntry,
    /// The target is not in the entry's `targets`.
    TargetNotAllowed,
    /// The entry's `max_per_hour` is spent.
    RateSpent,
    /// The requested lifetime is above the entry's `max_ttl`.
    TtlAboveMaximum,
    /// A constrained argument is missing, unreadable or outside the entry's bound.
    ArgOutsideBound,
    /// The entry's `max_active` blocks are already held.
    ActiveSpent,
}

impl CeilingStep {
    /// The sentence a refusal carries, naming the failing step.
    #[must_use]
    pub fn detail(self) -> &'static str {
        match self {
            Self::Disabled => "ceiling: unattended execution is not enabled on this host",
            Self::NoEntry => "ceiling: no unattended entry covers this capability",
            Self::TargetNotAllowed => "ceiling: this target is not in the unattended list",
            Self::RateSpent => "ceiling: the hourly unattended limit was reached",
            Self::TtlAboveMaximum => {
                "ceiling: the requested lifetime is above the unattended maximum"
            }
            Self::ArgOutsideBound => "ceiling: an argument is outside the unattended bound",
            Self::ActiveSpent => "ceiling: the unattended active limit was reached",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    contract: String,
    any_target: bool,
    targets: BTreeSet<String>,
    max_per_hour: u32,
    max_active: Option<u32>,
    max_ttl: Option<SignedDuration>,
    /// Argument key to the integer bound; `None` when the text is not an integer, which
    /// refuses every request that names the key.
    args: BTreeMap<String, Option<i64>>,
}

/// What a containment request asks of the ceiling beyond contract, target and rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirewallAsk {
    /// The lifetime the block has after `confirm`.
    pub ttl: SignedDuration,
    /// The IPv4 prefix length the request carries, as text parsed; `None` when unreadable.
    pub prefix_len_v4: Option<i64>,
    /// The IPv6 prefix length the request carries; `None` when unreadable.
    pub prefix_len_v6: Option<i64>,
}

/// The ceiling of this agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceiling {
    enabled: bool,
    entries: Vec<Entry>,
    summary: String,
}

impl Ceiling {
    /// The ceiling `section` describes.
    #[must_use]
    pub fn from_section(section: &UnattendedSection) -> Self {
        let entries: Vec<Entry> = section
            .allow
            .iter()
            .map(|allow| Entry {
                contract: allow.capability.clone(),
                any_target: allow.targets.iter().any(|t| t == "*"),
                targets: allow
                    .targets
                    .iter()
                    .filter(|t| *t != "*")
                    .cloned()
                    .collect(),
                max_per_hour: allow.max_per_hour,
                max_active: allow.max_active,
                max_ttl: allow.max_ttl.map(|ttl| ttl.as_signed()),
                args: allow
                    .args
                    .iter()
                    .flatten()
                    .map(|(key, values)| {
                        (
                            key.clone(),
                            values.first().and_then(|v| v.parse::<i64>().ok()),
                        )
                    })
                    .collect(),
            })
            .collect();
        let summary = CeilingSummary {
            enabled: section.enabled,
            entries: entries.iter().filter_map(summary_entry).collect(),
            hash: None,
        };
        Self {
            enabled: section.enabled,
            entries,
            // A serialisation failure would send an unreadable summary, which the hub refuses
            // as malformed: never permissive.
            summary: serde_json::to_string(&summary).unwrap_or_default(),
        }
    }

    /// The summary text sent in `hello` and `agent_status`.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// SHA-256 of the summary text, for change detection at the hub.
    #[must_use]
    pub fn hash(&self) -> Sha256Bytes {
        Sha256Bytes(Sha256::digest(self.summary.as_bytes()).into())
    }

    /// The ordered check for a policy-labelled request: enabled, an entry for the exact
    /// contract, the target in `targets`, and `max_per_hour` not spent by
    /// `accepted_last_hour` policy-labelled acceptances.
    pub fn check(
        &self,
        contract: &str,
        target: &str,
        accepted_last_hour: u32,
    ) -> Result<(), CeilingStep> {
        if !self.enabled {
            return Err(CeilingStep::Disabled);
        }
        let entry = self
            .entries
            .iter()
            .find(|e| e.contract == contract)
            .ok_or(CeilingStep::NoEntry)?;
        if !entry.any_target && !entry.targets.contains(target) {
            return Err(CeilingStep::TargetNotAllowed);
        }
        if accepted_last_hour >= entry.max_per_hour {
            return Err(CeilingStep::RateSpent);
        }
        Ok(())
    }

    /// The ordered check for a policy-labelled containment phase: enabled, an entry, the target,
    /// the lifetime and the prefix floors, then `max_per_hour` when `accepted_last_hour` is
    /// `Some` (only an `apply` spends the hour). A bound the entry states must be met by a
    /// value that parsed; the helper's own maxima still apply after this.
    pub fn check_firewall(
        &self,
        contract: &str,
        target: &str,
        ask: &FirewallAsk,
        accepted_last_hour: Option<u32>,
    ) -> Result<(), CeilingStep> {
        self.check(contract, target, 0)?;
        let entry = self
            .entries
            .iter()
            .find(|e| e.contract == contract)
            .ok_or(CeilingStep::NoEntry)?;
        if entry.max_ttl.is_some_and(|max| ask.ttl > max) {
            return Err(CeilingStep::TtlAboveMaximum);
        }
        for (key, asked) in [
            ("prefix_len_v4", ask.prefix_len_v4),
            ("prefix_len_v6", ask.prefix_len_v6),
        ] {
            if let Some(bound) = entry.args.get(key)
                && !matches!((asked, bound), (Some(asked), Some(bound)) if asked >= *bound)
            {
                return Err(CeilingStep::ArgOutsideBound);
            }
        }
        if let Some(accepted) = accepted_last_hour
            && accepted >= entry.max_per_hour
        {
            return Err(CeilingStep::RateSpent);
        }
        Ok(())
    }

    /// The `max_active` the entry for `contract` states, if any.
    #[must_use]
    pub fn max_active(&self, contract: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.contract == contract)
            .and_then(|e| e.max_active)
    }
}

fn summary_entry(entry: &Entry) -> Option<CeilingEntry> {
    let contract = ContractId::parse(&entry.contract).ok()?;
    let targets = if entry.any_target {
        CeilingTargets::Any
    } else {
        CeilingTargets::Names(
            entry
                .targets
                .iter()
                .map(|t| TargetName::parse(t).ok())
                .collect::<Option<_>>()?,
        )
    };
    Some(CeilingEntry {
        contract,
        targets,
        max_per_hour: entry.max_per_hour,
        max_active: entry.max_active,
        max_ttl: entry.max_ttl,
        // An unreadable bound drops the whole entry from the summary: the hub then sees no
        // coverage, never a bound that was not stated.
        args: entry
            .args
            .iter()
            .map(|(key, bound)| bound.map(|bound| (key.clone(), bound)))
            .collect::<Option<_>>()?,
    })
}
