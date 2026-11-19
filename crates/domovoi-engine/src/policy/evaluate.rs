//! The policy evaluator: one pure function from rules and a request to a decision.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use domovoi_core::capability::{EffectClass, FactId, TargetName};
use serde::{Deserialize, Serialize};

use super::condition::{Condition, FactStatus};
use super::decision::{Constraints, Decision, Overridden, Warning};
use super::error::PolicyError;
use super::limits;
use super::outcome::{ConfirmationRequirement, HardLimit, Outcome, OutcomeReason};
use super::request::{EvaluateRequest, MAX_FACTS};
use super::rule::{HostScope, Rule, RuleId, TargetScope};
use super::specificity::{ResolvedScope, is_strictly_more_specific, resolve_against};

/// Which rules take part: the enforced ones, or the shadow ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSelection {
    /// Rules with `shadow == false`.
    Enforced,
    /// Rules with `shadow == true`; the result is recorded and never enforced.
    Shadow,
}

/// The decision the enforced policy takes on this request. Pure and deterministic.
///
/// `Err` means the rule set or the request is malformed: fail closed by refusing to produce a
/// decision, never by producing one the caller might act on.
pub fn evaluate(rules: &[Rule], request: &EvaluateRequest) -> Result<Decision, PolicyError> {
    run(rules, request, RuleSelection::Enforced)
}

/// The decision the shadow rules take on the same request, through the same algorithm.
///
/// This is a separate entry point rather than a flag so a flipped boolean can never enforce
/// a shadow rule.
pub fn evaluate_shadow(rules: &[Rule], request: &EvaluateRequest) -> Result<Decision, PolicyError> {
    run(rules, request, RuleSelection::Shadow)
}

/// Validates a rule set. Cheap; `run` calls it, so an invalid set can never be evaluated.
pub fn validate_rules(rules: &[Rule]) -> Result<(), PolicyError> {
    let mut seen = HashSet::new();
    for rule in rules {
        if !seen.insert(&rule.id) {
            return Err(PolicyError::DuplicateRuleId(rule.id.clone()));
        }
    }
    for rule in rules {
        let id = || rule.id.clone();
        if rule.locked && rule.outcome != Outcome::Deny {
            return Err(PolicyError::LockedNotDeny {
                rule: id(),
                found: rule.outcome.name(),
            });
        }
        if matches!(rule.outcome, Outcome::Unavailable(_)) {
            return Err(PolicyError::UnavailableRuleOutcome { rule: id() });
        }
        if matches!(&rule.targets, TargetScope::Exact(names) if names.is_empty()) {
            return Err(PolicyError::EmptyTargetSet { rule: id() });
        }
        for condition in &rule.conditions {
            match condition {
                Condition::TimeWindow { start, end } if !start.is_valid() || !end.is_valid() => {
                    return Err(PolicyError::InvalidTimeOfDay { rule: id() });
                }
                Condition::MaxTtl { max_ttl } if max_ttl.is_negative() => {
                    return Err(PolicyError::NegativeMaxTtl { rule: id() });
                }
                Condition::TimeWindow { start, end } if start == end => {
                    return Err(PolicyError::DegenerateTimeWindow {
                        rule: id(),
                        start: start.label(),
                        end: end.label(),
                    });
                }
                Condition::ArgConstraint { arg, .. } if !valid_arg_key(arg) => {
                    return Err(PolicyError::BadArgKey {
                        rule: id(),
                        key: arg.clone(),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn valid_arg_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn validate_request(request: &EvaluateRequest) -> Result<(), PolicyError> {
    if request.contract.effect != EffectClass::Contain && !request.containment.is_empty() {
        return Err(PolicyError::ContainmentOnNonContain);
    }
    if request.expected_ceiling_hash.is_some() && request.ceiling.is_none() {
        return Err(PolicyError::CeilingHashMissing);
    }
    if request.facts.len() > MAX_FACTS {
        return Err(PolicyError::TooManyFacts {
            count: request.facts.len(),
            max: MAX_FACTS,
        });
    }
    Ok(())
}

/// A rule that matched, with its normalised scope.
struct Matched<'a> {
    rule: &'a Rule,
    scope: ResolvedScope,
}

type MatchSet<'a> = BTreeMap<RuleId, Matched<'a>>;

/// The single algorithm behind `evaluate`, `evaluate_shadow` and the simulator.
pub(crate) fn run(
    rules: &[Rule],
    request: &EvaluateRequest,
    selection: RuleSelection,
) -> Result<Decision, PolicyError> {
    // Stage 0: guard.
    validate_rules(rules)?;
    validate_request(request)?;
    // Stage 1: match.
    let matched = select(rules, request, selection);
    // Stages 3-6: the ordinary resolution.
    let ordinary = resolve(&matched, request);
    // Stage 2: a locked deny beats any ordinary result and every hard limit.
    let locked: Vec<RuleId> = matched
        .iter()
        .filter(|(_, m)| m.rule.locked && m.rule.outcome == Outcome::Deny)
        .map(|(id, _)| id.clone())
        .collect();
    if !locked.is_empty() {
        return Ok(locked_decision(ordinary, locked));
    }
    // Stage 7: hard limits.
    Ok(with_limits(ordinary, &matched, request))
}

fn select<'a>(
    rules: &'a [Rule],
    request: &EvaluateRequest,
    selection: RuleSelection,
) -> MatchSet<'a> {
    let mut matched = BTreeMap::new();
    for rule in rules {
        if !rule.enabled || (selection == RuleSelection::Shadow) != rule.shadow {
            continue;
        }
        let host_ok = match &rule.host {
            HostScope::All => true,
            HostScope::Host(h) => *h == request.host,
        };
        let target_ok = match &rule.targets {
            TargetScope::Any => true,
            TargetScope::Exact(names) => names.contains(&request.target),
        };
        if !host_ok || !target_ok {
            continue;
        }
        let Some(scope) = resolve_against(rule, &request.contract) else {
            continue;
        };
        if !rule.conditions.iter().all(|c| c.holds(request)) {
            continue;
        }
        matched.insert(rule.id.clone(), Matched { rule, scope });
    }
    matched
}

fn facts_used(matched: &MatchSet<'_>, request: &EvaluateRequest) -> Vec<(FactId, FactStatus)> {
    let mut ids: BTreeSet<&FactId> = request.contract.safety_facts.iter().collect();
    for m in matched.values() {
        for condition in &m.rule.conditions {
            if let Condition::RequiresFact { fact, .. } = condition {
                ids.insert(fact);
            }
        }
    }
    ids.into_iter()
        .map(|fact| {
            let status = request
                .facts
                .get(fact)
                .map_or(FactStatus::Unknown, |s| s.status());
            (fact.clone(), status)
        })
        .collect()
}

fn default_outcome(effect: EffectClass) -> Outcome {
    match effect {
        EffectClass::Read => Outcome::Allow,
        EffectClass::Mutate | EffectClass::Contain | EffectClass::Destructive => {
            Outcome::RequireApproval
        }
    }
}

fn typed_for(outcome: Outcome, destructive: bool, safety_limit: bool) -> ConfirmationRequirement {
    if outcome == Outcome::RequireApproval && (destructive || safety_limit) {
        ConfirmationRequirement::Typed
    } else {
        ConfirmationRequirement::None
    }
}

/// Stages 3-6: the outcome of the matching rules with no hard limit applied.
fn resolve(matched: &MatchSet<'_>, request: &EvaluateRequest) -> Decision {
    let effect = request.contract.effect;
    let mut decision = Decision {
        outcome: default_outcome(effect),
        reason: OutcomeReason::Default,
        matched: matched.keys().cloned().collect(),
        winners: Vec::new(),
        locked: Vec::new(),
        overridden: Vec::new(),
        would_have_been: None,
        constraints: Constraints::default(),
        hard_limits_applied: Vec::new(),
        facts_used: facts_used(matched, request),
        typed_confirmation: ConfirmationRequirement::None,
        warnings: Vec::new(),
        policy_version: request.policy_version,
        ceiling_hash: request.ceiling.as_ref().and_then(|c| c.hash.clone()),
    };
    if !matched.is_empty() {
        // Stage 4: the maximal rules.
        let winners: Vec<(&RuleId, &Matched<'_>)> = matched
            .iter()
            .filter(|(id, m)| {
                !matched
                    .iter()
                    .any(|(other, o)| other != *id && is_strictly_more_specific(&o.scope, &m.scope))
            })
            .collect();
        decision.winners = winners.iter().map(|(id, _)| (*id).clone()).collect();
        for (bid, b) in matched {
            for (wid, w) in &winners {
                if is_strictly_more_specific(&w.scope, &b.scope) {
                    decision.overridden.push(Overridden {
                        broader: bid.clone(),
                        by: (*wid).clone(),
                    });
                    if w.rule.outcome == Outcome::Allow && b.rule.outcome == Outcome::Deny {
                        decision.warnings.push(Warning::DangerousException {
                            allow: (*wid).clone(),
                            deny: bid.clone(),
                        });
                    }
                }
            }
        }
        decision.overridden.sort();
        decision.warnings.sort();
        // Stages 5 and 6: one outcome.
        let winner_rules: Vec<&Matched<'_>> = winners.iter().map(|(_, m)| *m).collect();
        let first = winner_rules[0].rule.outcome;
        if winner_rules.iter().all(|w| w.rule.outcome == first) {
            decision.outcome = first;
            decision.reason = OutcomeReason::SingleWinner;
            if first == Outcome::Allow {
                decision.constraints = allow_constraints(&winner_rules, request);
            }
        } else if winner_rules
            .iter()
            .all(|w| w.scope == winner_rules[0].scope)
        {
            decision.outcome = Outcome::Deny;
            decision.reason = OutcomeReason::EqualScopeDeny;
        } else {
            decision.reason = OutcomeReason::IncomparableFallback;
            decision.outcome = if winner_rules.iter().any(|w| w.rule.outcome == Outcome::Deny) {
                Outcome::Deny
            } else {
                Outcome::RequireApproval
            };
        }
    }
    decision.typed_confirmation =
        typed_for(decision.outcome, effect == EffectClass::Destructive, false);
    decision
}

/// The intersection of the winners' bounds, carried to the permit.
///
/// `Any` is the identity of the target intersection: it never erases an `Exact` restriction
/// another winner states, so `targets` is `None` only when every winner is `Any`.
///
/// The target intersection always contains the request's target, because every winner
/// matched and a rule only matches a target its scope contains.
fn allow_constraints(winners: &[&Matched<'_>], request: &EvaluateRequest) -> Constraints {
    let mut rate = request.requested_rate_per_hour;
    let mut ttl = request.requested_ttl;
    let mut targets: Option<BTreeSet<TargetName>> = None;
    let mut conditions = Vec::new();
    for w in winners {
        for condition in &w.rule.conditions {
            match condition {
                Condition::MaxRate { max_per_hour } => {
                    rate = Some(rate.map_or(*max_per_hour, |r| r.min(*max_per_hour)));
                }
                Condition::MaxTtl { max_ttl } => {
                    ttl = Some(ttl.map_or(*max_ttl, |t| t.min(*max_ttl)));
                }
                _ => {}
            }
            conditions.push(condition.clone());
        }
        match &w.rule.targets {
            TargetScope::Any => {}
            TargetScope::Exact(names) => {
                targets = Some(match targets {
                    None => names.clone(),
                    Some(acc) => acc.intersection(names).cloned().collect(),
                });
            }
        }
    }
    Constraints {
        max_per_hour: rate,
        max_ttl: ttl,
        targets,
        conditions,
    }
}

/// Stage 2: a locked `Deny` is a hard prohibition no narrower rule and no limit can weaken.
fn locked_decision(ordinary: Decision, locked: Vec<RuleId>) -> Decision {
    let mut overridden = Vec::new();
    for winner in &ordinary.winners {
        for lock in &locked {
            if winner != lock {
                overridden.push(Overridden {
                    broader: winner.clone(),
                    by: lock.clone(),
                });
            }
        }
    }
    overridden.sort();
    Decision {
        outcome: Outcome::Deny,
        reason: OutcomeReason::LockedDeny,
        matched: ordinary.matched.clone(),
        winners: Vec::new(),
        locked,
        overridden,
        constraints: Constraints::default(),
        hard_limits_applied: Vec::new(),
        facts_used: ordinary.facts_used.clone(),
        typed_confirmation: ConfirmationRequirement::None,
        warnings: Vec::new(),
        policy_version: ordinary.policy_version,
        ceiling_hash: ordinary.ceiling_hash.clone(),
        would_have_been: Some(Box::new(ordinary)),
    }
}

/// Stage 7: applies the hard limits to the ordinary decision.
fn with_limits(ordinary: Decision, matched: &MatchSet<'_>, request: &EvaluateRequest) -> Decision {
    let names: BTreeSet<&TargetName> = ordinary
        .winners
        .iter()
        .filter_map(|id| matched.get(id))
        .filter(|m| m.rule.outcome == Outcome::Allow)
        .filter_map(|m| match &m.rule.targets {
            TargetScope::Exact(names) => Some(names),
            TargetScope::Any => None,
        })
        .flatten()
        .collect();
    let (outcome, recorded) = limits::apply(request, ordinary.outcome, &names);
    let safety_limit = recorded.contains(&HardLimit::ContainmentUnverified)
        || recorded.contains(&HardLimit::ManagementPathUncovered);
    let mut decision = ordinary.clone();
    if outcome == Outcome::RequireApproval {
        if recorded.contains(&HardLimit::ContainmentUnverified) {
            decision.warnings.push(Warning::SafetySetUnverified);
        }
        if recorded.contains(&HardLimit::ManagementPathUncovered) {
            let mut paths = request.containment.uncovered_paths.clone();
            paths.sort();
            paths.dedup();
            decision
                .warnings
                .push(Warning::ManagementPathsUnprotected { paths });
        }
        decision.warnings.sort();
        decision.warnings.dedup();
    }
    decision.typed_confirmation = typed_for(
        outcome,
        request.contract.effect == EffectClass::Destructive,
        safety_limit,
    );
    if outcome != Outcome::Allow {
        decision.constraints = Constraints::default();
    }
    decision.hard_limits_applied = recorded;
    if outcome != ordinary.outcome {
        decision.would_have_been = Some(Box::new(ordinary));
    }
    decision.outcome = outcome;
    decision
}
