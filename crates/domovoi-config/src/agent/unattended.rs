//! `[unattended]` in `agent.toml`: the ceiling on what a policy-labelled request may do.
//!
//! Local and root-owned. The hub cannot send, widen or override it.

use std::collections::BTreeMap;

use domovoi_core::EffectClass;
use domovoi_core::capability::containment::BLOCK_SOURCE_ID;
use domovoi_core::capability::{POLICY_DISPATCHABLE, known_contracts};
use domovoi_core::containment::Net;
use domovoi_core::unit::UnitId;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;
use crate::issue::ConfigIssue;

/// `[unattended]`. With no section, no `policy`-labelled request is accepted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnattendedSection {
    /// Master switch. When `false` every policy-labelled request is refused.
    pub enabled: bool,
    /// What may run unattended, one entry per contract.
    #[serde(default)]
    pub allow: Vec<UnattendedAllow>,
}

/// One `[[unattended.allow]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnattendedAllow {
    /// The exact contract id, such as `unit.restart`. No wildcard.
    pub capability: String,
    /// Canonical targets: unit ids for `unit.restart`, `ipnet:<canonical network>` for
    /// `firewall.block_source`. `"*"` must be written explicitly.
    pub targets: Vec<String>,
    /// How many policy-labelled executions of this contract per rolling hour. At least 1.
    pub max_per_hour: u32,
    /// For contracts that bound concurrent runs (`firewall.block_source`: blocks held at
    /// once, 1 to 10000). Not accepted for `unit.restart`.
    #[serde(default)]
    pub max_active: Option<u32>,
    /// For contracts that bound a lifetime (`firewall.block_source`: above 30 seconds, at
    /// most 365 days). Not accepted for `unit.restart`.
    #[serde(default)]
    pub max_ttl: Option<Duration>,
    /// Constrained argument values (`firewall.block_source`: `prefix_len_v4` and
    /// `prefix_len_v6`, one shortest allowed prefix length each). Not accepted for
    /// `unit.restart`.
    #[serde(default)]
    pub args: Option<BTreeMap<String, Vec<String>>>,
}

impl UnattendedSection {
    /// Reports every problem with the section, under `unattended`.
    pub(crate) fn validate(&self, issues: &mut Vec<ConfigIssue>) {
        let contracts = known_contracts();
        for (index, entry) in self.allow.iter().enumerate() {
            let at = |field: &str| format!("unattended.allow[{index}].{field}");
            let known = contracts.iter().find(|c| c.id.as_str() == entry.capability);
            match known {
                None => issues.push(ConfigIssue::error(
                    at("capability"),
                    "is not a contract this agent knows",
                )),
                Some(c)
                    if !matches!(c.effect, EffectClass::Mutate | EffectClass::Contain)
                        || !POLICY_DISPATCHABLE.contains(&entry.capability.as_str()) =>
                {
                    issues.push(ConfigIssue::error(
                        at("capability"),
                        "cannot run unattended: only a listed write capability may",
                    ));
                }
                Some(_) => {}
            }
            if entry.targets.is_empty() {
                issues.push(ConfigIssue::error(
                    at("targets"),
                    "needs at least one target; write \"*\" to allow every target",
                ));
            }
            let firewall = entry.capability == BLOCK_SOURCE_ID;
            for target in &entry.targets {
                if target == "*" {
                    if !firewall {
                        issues.push(ConfigIssue::warning(
                            at("targets"),
                            "\"*\" lets policy restart every unit on this host unattended; \
                             name the units instead",
                        ));
                    }
                    continue;
                }
                if firewall {
                    if !is_canonical_network_target(target) {
                        issues.push(ConfigIssue::error(
                            at("targets"),
                            format!(
                                "`{target}` is not a canonical network such as \"ipnet:203.0.113.0/24\""
                            ),
                        ));
                    }
                } else if UnitId::parse(target).is_err() {
                    issues.push(ConfigIssue::error(
                        at("targets"),
                        format!(
                            "`{target}` is not a canonical unit id such as \"example.service\""
                        ),
                    ));
                }
            }
            if entry.max_per_hour == 0 {
                issues.push(ConfigIssue::error(at("max_per_hour"), "must be at least 1"));
            }
            if firewall {
                validate_firewall(entry, &at, issues);
                continue;
            }
            // `unit.restart` declares no constrainable argument and no lifetime.
            for (present, field) in [
                (entry.max_active.is_some(), "max_active"),
                (entry.max_ttl.is_some(), "max_ttl"),
                (entry.args.is_some(), "args"),
            ] {
                if present {
                    issues.push(ConfigIssue::error(
                        at(field),
                        format!("is not accepted for {}", entry.capability),
                    ));
                }
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, entry) in self.allow.iter().enumerate() {
            if !seen.insert(entry.capability.as_str()) {
                issues.push(ConfigIssue::error(
                    format!("unattended.allow[{index}].capability"),
                    "is listed twice; use one entry per contract",
                ));
            }
        }
    }
}

/// Whether `target` is `ipnet:` followed by the canonical text of one network.
fn is_canonical_network_target(target: &str) -> bool {
    target
        .strip_prefix("ipnet:")
        .and_then(|text| Net::parse(text).ok().map(|net| net.canonical() == text))
        .unwrap_or(false)
}

/// The bounds a `firewall.block_source` entry may state.
fn validate_firewall(
    entry: &UnattendedAllow,
    at: &dyn Fn(&str) -> String,
    issues: &mut Vec<ConfigIssue>,
) {
    if let Some(active) = entry.max_active
        && !(1..=10_000).contains(&active)
    {
        issues.push(ConfigIssue::error(at("max_active"), "must be 1 to 10000"));
    }
    if let Some(ttl) = entry.max_ttl {
        let secs = ttl.as_signed().as_secs();
        if !(31..=31_536_000).contains(&secs) {
            issues.push(ConfigIssue::error(
                at("max_ttl"),
                "must be above 30 seconds and at most 365 days",
            ));
        }
    }
    for (key, values) in entry.args.iter().flatten() {
        let longest = match key.as_str() {
            "prefix_len_v4" => 32,
            "prefix_len_v6" => 128,
            _ => {
                issues.push(ConfigIssue::error(
                    at("args"),
                    format!("`{key}` is not a constrainable argument of {BLOCK_SOURCE_ID}"),
                ));
                continue;
            }
        };
        let valid = matches!(
            values.as_slice(),
            [one] if one.parse::<i64>().is_ok_and(|n| (1..=longest).contains(&n))
        );
        if !valid {
            issues.push(ConfigIssue::error(
                at("args"),
                format!("`{key}` needs exactly one prefix length from 1 to {longest}"),
            ));
        }
    }
}
