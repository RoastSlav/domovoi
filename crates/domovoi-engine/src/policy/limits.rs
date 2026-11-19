//! The ten hard limits, applied after rule resolution.

use std::collections::BTreeSet;

use domovoi_core::capability::{CeilingSummary, CeilingTargets, EffectClass, TargetName};

use super::condition::{FactState, satisfied};
use super::outcome::{HardLimit, Outcome, UnavailableReason};
use super::request::EvaluateRequest;

/// The running result of the limits.
struct LimitState {
    outcome: Outcome,
    recorded: Vec<HardLimit>,
}

/// The one rule every limit uses.
///
/// `record` is separate from raising because a limit can be worth showing even when the
/// outcome is already at least as severe. The outcome only ever moves up
/// (`Allow < RequireApproval < Unavailable < Deny`): a limit can never lower a result, turn a
/// `Deny` into anything else, or produce `Allow`.
fn apply_limit(state: &mut LimitState, limit: HardLimit, candidate: Outcome, record: bool) {
    if record {
        state.recorded.push(limit);
    }
    if record && candidate > state.outcome {
        state.outcome = candidate;
    }
}

/// Runs the ten limits in order over the ordinary outcome.
///
/// `winning_allow_names` are the target names of the winning `Allow` rules, which widen the
/// pinned set for this request only. Returns the final outcome and the limits that applied,
/// in declaration order.
pub(super) fn apply(
    request: &EvaluateRequest,
    ordinary: Outcome,
    winning_allow_names: &BTreeSet<&TargetName>,
) -> (Outcome, Vec<HardLimit>) {
    let mut state = LimitState {
        outcome: ordinary,
        recorded: Vec::new(),
    };
    let allow = ordinary == Outcome::Allow;
    let effect = request.contract.effect;
    let contain = effect == EffectClass::Contain;
    let approval = Outcome::RequireApproval;
    let c = &request.containment;

    apply_limit(
        &mut state,
        HardLimit::DestructiveNeverAllowed,
        approval,
        allow && effect == EffectClass::Destructive,
    );
    apply_limit(
        &mut state,
        HardLimit::TargetNotPinned,
        approval,
        allow && !target_pinned(request, winning_allow_names),
    );
    apply_limit(
        &mut state,
        HardLimit::SafetyFactUnknown,
        approval,
        allow && safety_fact_unknown(request),
    );
    apply_limit(
        &mut state,
        HardLimit::CeilingDoesNotCover,
        approval,
        allow
            && !ceiling_covers(
                request.ceiling.as_ref(),
                request.expected_ceiling_hash.as_deref(),
                request,
            ),
    );
    apply_limit(
        &mut state,
        HardLimit::ContainmentUnverified,
        approval,
        contain && !c.attested,
    );
    apply_limit(
        &mut state,
        HardLimit::ManagementPathUncovered,
        approval,
        contain && (!c.uncovered_paths.is_empty() || c.stale_confirmation),
    );
    apply_limit(
        &mut state,
        HardLimit::ContainmentTargetExcluded,
        Outcome::Deny,
        c.target_overlaps_verified_set,
    );
    apply_limit(
        &mut state,
        HardLimit::HelperSafetyIncomplete,
        Outcome::Unavailable(UnavailableReason::SafetyUnknown),
        c.helper_incomplete,
    );
    apply_limit(
        &mut state,
        HardLimit::AutonomyInactive,
        approval,
        request.autonomy.paused || request.autonomy.circuit_broken,
    );
    apply_limit(
        &mut state,
        HardLimit::AgentRunsAsRoot,
        approval,
        allow && request.runs_as_root,
    );
    (state.outcome, state.recorded)
}

/// Whether the target is pinned, or named by a winning `Allow` rule and resolved for this
/// request. A name that did not resolve contributes nothing, so it cannot widen anything.
fn target_pinned(request: &EvaluateRequest, names: &BTreeSet<&TargetName>) -> bool {
    request.pinned.contains(&request.target_identity)
        || names
            .iter()
            .filter_map(|name| request.rule_targets.get(*name))
            .any(|identity| *identity == request.target_identity)
}

/// Whether a declared safety fact is anything but `Known`. An absent fact counts as unknown.
fn safety_fact_unknown(request: &EvaluateRequest) -> bool {
    request
        .contract
        .safety_facts
        .iter()
        .any(|fact| !matches!(request.facts.get(fact), Some(FactState::Known(_))))
}

/// Whether the host's ceiling covers the request. Every failure, including a missing summary,
/// a disabled ceiling and a hash disagreement, answers `false`: a ceiling that cannot be
/// verified never allows.
pub(super) fn ceiling_covers(
    ceiling: Option<&CeilingSummary>,
    expected_hash: Option<&str>,
    request: &EvaluateRequest,
) -> bool {
    let Some(summary) = ceiling else {
        return false;
    };
    if !summary.enabled {
        return false;
    }
    if let Some(expected) = expected_hash
        && summary.hash.as_deref() != Some(expected)
    {
        return false;
    }
    summary
        .entries
        .iter()
        .filter(|entry| entry.contract == request.contract.id)
        .any(|entry| {
            let targets = match &entry.targets {
                CeilingTargets::Any => true,
                CeilingTargets::Names(names) => names.contains(&request.target),
            };
            let rate = request
                .requested_rate_per_hour
                .is_none_or(|r| r <= entry.max_per_hour);
            let ttl = match (request.requested_ttl, entry.max_ttl) {
                (Some(t), Some(max)) => t <= max,
                _ => true,
            };
            let args = request.contract.constrainable_args.iter().all(|arg| {
                let Some(bound) = entry.args.get(&arg.key) else {
                    return true;
                };
                request
                    .args
                    .get(&arg.key)
                    .and_then(|text| text.parse::<i64>().ok())
                    .is_some_and(|n| satisfied(arg.kind, n, *bound))
            });
            targets && rate && ttl && args
        })
}
