//! `[unattended]` in `agent.toml`: the ceiling on what a policy-labelled request may do.
//!
//! Local and root-owned. The hub cannot send, widen or override it.

use std::collections::BTreeMap;

use domovoi_core::EffectClass;
use domovoi_core::capability::{POLICY_DISPATCHABLE, known_contracts};
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
    /// Canonical targets (unit ids for `unit.restart`). `"*"` must be written explicitly.
    pub targets: Vec<String>,
    /// How many policy-labelled executions of this contract per rolling hour. At least 1.
    pub max_per_hour: u32,
    /// For contracts that bound concurrent runs. Not accepted for `unit.restart`.
    #[serde(default)]
    pub max_active: Option<u32>,
    /// For contracts that bound a lifetime. Not accepted for `unit.restart`.
    #[serde(default)]
    pub max_ttl: Option<Duration>,
    /// Constrained argument values. Not accepted for `unit.restart`.
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
                    if c.effect != EffectClass::Mutate
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
            for target in &entry.targets {
                if target != "*" && UnitId::parse(target).is_err() {
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
