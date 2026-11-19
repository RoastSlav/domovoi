//! Publication review: conflicts an owner must see before a draft becomes the policy.

use std::collections::BTreeMap;

use domovoi_core::HostId;
use domovoi_core::capability::{CeilingSummary, CeilingTargets, ContractIndex, TargetName};

use super::error::PolicyError;
use super::evaluate::validate_rules;
use super::outcome::Outcome;
use super::rule::{CapabilityScope, HostScope, Rule, RuleId, TargetScope};
use super::specificity::{ResolvedScope, overlaps, resolve_in_index, scope_contains};

/// One finding of a publication review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What kind of finding this is.
    pub kind: FindingKind,
    /// The first rule involved. Equal to `second` only for the ceiling note.
    pub first: RuleId,
    /// The second rule involved.
    pub second: RuleId,
    /// What the pair resolves to at runtime, when that is meaningful.
    pub resolution: Option<Outcome>,
    /// Whether either rule is a shadow rule. A shadow pair never needs an acknowledgement.
    pub shadow: bool,
    /// Owner-facing, one sentence.
    pub note: String,
    /// Whether publishing must wait for the owner to acknowledge this finding.
    pub requires_acknowledgement: bool,
}

/// The kinds of finding a review produces, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    /// Equal resolved scope, differing outcomes: resolves to `Deny`.
    EqualScopeConflict,
    /// Overlapping incomparable rules, differing outcomes: resolves to the safer outcome.
    IncomparableConflict,
    /// An enabled rule that a locked `Deny` makes ineffective.
    IneffectiveUnderLock,
    /// An `Allow` naming a target no host's ceiling covers. Informational.
    TargetOutsideEveryCeiling,
}

/// Everything a publication review found, worst first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublicationReport {
    /// The findings, sorted by kind and then by the rules involved.
    pub findings: Vec<Finding>,
}

impl PublicationReport {
    /// Whether publishing must wait for acknowledgements.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.findings.iter().any(|f| f.requires_acknowledgement)
    }

    /// The findings that block, for the acknowledgement screen.
    pub fn blocking(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.requires_acknowledgement)
    }
}

/// Reviews a draft rule set before publication, using the same specificity helpers as runtime.
///
/// A rule naming a contract or category the index does not know refuses the review; an
/// effect is never guessed. An empty `ceilings` map produces no ceiling findings: nothing is
/// known, so nothing is claimed.
pub fn validate_publication(
    rules: &[Rule],
    index: &ContractIndex,
    ceilings: &BTreeMap<HostId, CeilingSummary>,
) -> Result<PublicationReport, PolicyError> {
    validate_rules(rules)?;
    let mut resolved: Vec<(&Rule, ResolvedScope)> = Vec::new();
    for rule in rules {
        resolved.push((rule, resolve_in_index(rule, index)?));
    }
    resolved.retain(|(rule, _)| rule.enabled);
    resolved.sort_by(|a, b| a.0.id.cmp(&b.0.id));

    let mut findings = Vec::new();
    pair_findings(&resolved, &mut findings);
    lock_findings(&resolved, &mut findings);
    ceiling_findings(&resolved, index, ceilings, &mut findings);
    findings.sort_by(|a, b| {
        (a.kind, &a.first, &a.second, &a.note).cmp(&(b.kind, &b.first, &b.second, &b.note))
    });
    Ok(PublicationReport { findings })
}

fn pair_findings(resolved: &[(&Rule, ResolvedScope)], out: &mut Vec<Finding>) {
    for (i, (a, sa)) in resolved.iter().enumerate() {
        for (b, sb) in &resolved[i + 1..] {
            if a.outcome == b.outcome {
                continue;
            }
            let shadow = a.shadow || b.shadow;
            let a_in_b = scope_contains(sb, sa);
            let b_in_a = scope_contains(sa, sb);
            let (kind, resolution, note) = if a_in_b && b_in_a {
                (
                    FindingKind::EqualScopeConflict,
                    Outcome::Deny,
                    "These rules cover exactly the same scope and disagree, so the request is denied.",
                )
            } else if !a_in_b && !b_in_a && overlaps(sa, sb) {
                let safer = if a.outcome == Outcome::Deny || b.outcome == Outcome::Deny {
                    Outcome::Deny
                } else {
                    Outcome::RequireApproval
                };
                (
                    FindingKind::IncomparableConflict,
                    safer,
                    "These rules overlap and neither is more specific, so the safer outcome applies where both match.",
                )
            } else {
                continue;
            };
            out.push(Finding {
                kind,
                first: a.id.clone(),
                second: b.id.clone(),
                resolution: Some(resolution),
                shadow,
                note: note.to_owned(),
                requires_acknowledgement: !shadow,
            });
        }
    }
}

fn lock_findings(resolved: &[(&Rule, ResolvedScope)], out: &mut Vec<Finding>) {
    // A lock with conditions only applies where they hold, so it guarantees nothing.
    for (lock, lock_scope) in resolved
        .iter()
        .filter(|(r, _)| r.locked && r.conditions.is_empty())
    {
        for (rule, scope) in resolved {
            if rule.id == lock.id
                || rule.outcome == Outcome::Deny
                || !scope_contains(lock_scope, scope)
            {
                continue;
            }
            let shadow = lock.shadow || rule.shadow;
            out.push(Finding {
                kind: FindingKind::IneffectiveUnderLock,
                first: lock.id.clone(),
                second: rule.id.clone(),
                resolution: Some(Outcome::Deny),
                shadow,
                note: "An unconditional locked deny covers this rule, so it can never take effect."
                    .to_owned(),
                requires_acknowledgement: !shadow,
            });
        }
    }
}

fn ceiling_findings(
    resolved: &[(&Rule, ResolvedScope)],
    index: &ContractIndex,
    ceilings: &BTreeMap<HostId, CeilingSummary>,
    out: &mut Vec<Finding>,
) {
    if ceilings.is_empty() {
        return;
    }
    for (rule, _) in resolved {
        let TargetScope::Exact(names) = &rule.targets else {
            continue;
        };
        if rule.outcome != Outcome::Allow {
            continue;
        }
        for name in names {
            if covered_somewhere(rule, name, index, ceilings) {
                continue;
            }
            out.push(Finding {
                kind: FindingKind::TargetOutsideEveryCeiling,
                first: rule.id.clone(),
                second: rule.id.clone(),
                resolution: None,
                shadow: rule.shadow,
                note: format!(
                    "No host's unattended ceiling covers {name}, so allowing it will still ask for approval."
                ),
                requires_acknowledgement: false,
            });
        }
    }
}

fn covered_somewhere(
    rule: &Rule,
    name: &TargetName,
    index: &ContractIndex,
    ceilings: &BTreeMap<HostId, CeilingSummary>,
) -> bool {
    ceilings.iter().any(|(host, summary)| {
        let host_ok = match &rule.host {
            HostScope::All => true,
            HostScope::Host(h) => h == host,
        };
        host_ok
            && summary.enabled
            && summary.entries.iter().any(|entry| {
                let in_scope = match &rule.capability {
                    CapabilityScope::Any => true,
                    CapabilityScope::Contract(id) => *id == entry.contract,
                    CapabilityScope::Category(c) => {
                        index.get(&entry.contract).is_some_and(|k| k.category == *c)
                    }
                    CapabilityScope::Effect(e) => {
                        index.get(&entry.contract).is_some_and(|k| k.effect == *e)
                    }
                };
                in_scope
                    && match &entry.targets {
                        CeilingTargets::Any => true,
                        CeilingTargets::Names(names) => names.contains(name),
                    }
            })
    })
}
