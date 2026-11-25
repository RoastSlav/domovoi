//! Turns one Execution step into evaluator inputs, substituting the fail-closed value for
//! every input the hub cannot supply yet and recording each substitution as a gap.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::HostId;
use domovoi_core::capability::{ContractVersion, EffectClass, FactId, TargetKind, TargetName};
use domovoi_core::policy_store::ShadowCandidate;
use jiff::Timestamp;

use super::env::{
    AutonomyReading, CeilingReading, ContainmentReading, HostTarget, ShadowEnvironment,
};
use super::recorded::{ContractDoc, GapInput, GapReason, InputGap, RecordedInputs};
use crate::policy::{
    AutonomyState, CanonicalArgs, Condition, ContainmentInput, EvaluateRequest, Rule,
    names_to_resolve,
};

/// What `build` concluded about a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Evaluate it.
    Evaluate,
    /// Never evaluated; the code is stored as the row's reason.
    Skip(&'static str),
    /// The inputs could not be established; the code is stored as the row's reason.
    Unevaluable(&'static str),
}

/// The result of building the inputs for one step.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The request to evaluate; set exactly when the verdict is `Evaluate`.
    pub request: Option<EvaluateRequest>,
    /// The inputs as they are stored; set exactly when the verdict is `Evaluate`.
    pub inputs: Option<RecordedInputs>,
    /// Every substituted input, sorted and without duplicates.
    pub gaps: Vec<InputGap>,
    /// What to do with the step.
    pub verdict: Verdict,
}

impl Built {
    fn refused(verdict: Verdict) -> Self {
        Self {
            request: None,
            inputs: None,
            gaps: Vec::new(),
            verdict,
        }
    }
}

/// The prefix of the contracts that are never policy-routed: package installs are
/// standalone jobs, not Execution steps.
const PACKAGE_PREFIX: &str = "agent.package.";

/// Builds the inputs for `candidate` under `rules` at `now`. Never fails, never reads a
/// clock, never writes and never retries.
pub async fn build(
    candidate: &ShadowCandidate,
    rules: &[Rule],
    env: &dyn ShadowEnvironment,
    now: Timestamp,
) -> Built {
    if candidate.contract.as_str().starts_with(PACKAGE_PREFIX) {
        return Built::refused(Verdict::Skip("package_contract"));
    }
    let host = candidate.host;
    let Some(meta) = env.contract(host, &candidate.contract).await else {
        return Built::refused(Verdict::Unevaluable("contract_unknown"));
    };
    let Ok(host_target) = env.host_target(host).await else {
        return Built::refused(Verdict::Unevaluable("target_unresolved"));
    };
    // A host-level contract acts on the host; a unit contract on the unit the helper resolved.
    let target = match meta.target_kind {
        TargetKind::Host => host_target.clone(),
        TargetKind::Unit => {
            let Some(typed) = candidate
                .args
                .get("unit")
                .and_then(|text| TargetName::parse(text).ok())
            else {
                return Built::refused(Verdict::Unevaluable("target_unresolved"));
            };
            match env.unit_target(host, &typed).await {
                Ok(unit) => unit,
                Err(_) => return Built::refused(Verdict::Unevaluable("target_unresolved")),
            }
        }
        _ => return Built::refused(Verdict::Unevaluable("target_kind_unsupported")),
    };
    let Ok(version) = ContractVersion::new(candidate.contract_version) else {
        return Built::refused(Verdict::Unevaluable("contract_version_invalid"));
    };

    let mut gaps = Vec::new();
    let args = canonical_args(candidate);
    let pinned = pinned_identities(
        candidate,
        env,
        meta.target_kind == TargetKind::Unit,
        &mut gaps,
    )
    .await;
    let rule_targets = rule_targets(rules, &target);

    let ceiling = match env.ceiling(host).await {
        CeilingReading::Reported(summary) => Some(summary),
        CeilingReading::Absent => {
            gaps.push(gap(GapInput::Ceiling, GapReason::NotProducedYet));
            None
        }
        CeilingReading::HostNotConnected => {
            gaps.push(gap(GapInput::Ceiling, GapReason::HostNotConnected));
            None
        }
    };

    let wanted = wanted_facts(&meta.safety_facts, rules);
    let known = env.facts(host, &wanted).await;
    if wanted.iter().any(|fact| !known.contains_key(fact)) {
        gaps.push(gap(GapInput::Facts, GapReason::NotProducedYet));
    }

    // Unknown means root: the safe direction.
    let runs_as_root = match env.runs_as_root(host).await {
        Some(root) => root,
        None => {
            gaps.push(gap(GapInput::RunsAsRoot, GapReason::HostNotConnected));
            true
        }
    };

    let containment = if meta.effect == EffectClass::Contain {
        gaps.push(gap(GapInput::Containment, GapReason::NotProducedYet));
        // A helper-dependent contract fails closed rather than reading as attested. The
        // environment is asked so a future verified reading has one place to arrive.
        match env.containment(host).await {
            ContainmentReading::NotApplicable | ContainmentReading::Unavailable => {
                ContainmentInput {
                    helper_incomplete: true,
                    ..ContainmentInput::default()
                }
            }
        }
    } else {
        ContainmentInput::not_applicable()
    };

    // The pause switch is read; the circuit breaker is applied by the permit issuer, per
    // rule, so a step is evaluated here as not broken.
    let AutonomyReading::Reported(facts) = env.autonomy().await;
    let autonomy = AutonomyState {
        paused: facts.paused,
        circuit_broken: false,
    };

    gaps.sort();
    gaps.dedup();

    let request = EvaluateRequest {
        host,
        contract: meta.clone(),
        contract_version: version,
        target: target.name.clone(),
        target_identity: target.identity.clone(),
        args: args.clone(),
        requested_rate_per_hour: None,
        requested_ttl: None,
        pinned: pinned.clone(),
        rule_targets: rule_targets.clone(),
        ceiling: ceiling.clone(),
        expected_ceiling_hash: None,
        facts: known.clone(),
        runs_as_root,
        containment: containment.clone(),
        autonomy,
        now,
        policy_version: None,
    };
    let inputs = RecordedInputs {
        host,
        contract: ContractDoc::from_meta(&meta),
        contract_version: candidate.contract_version,
        target: target.name.to_string(),
        target_identity: target.identity.to_string(),
        args: args.canonical().to_owned(),
        requested_rate_per_hour: None,
        requested_ttl_secs: None,
        pinned: pinned.iter().map(ToString::to_string).collect(),
        rule_targets: rule_targets
            .iter()
            .map(|(name, id)| (name.to_string(), id.to_string()))
            .collect(),
        ceiling,
        facts: known
            .iter()
            .map(|(fact, state)| (fact.to_string(), *state))
            .collect(),
        runs_as_root,
        containment,
        autonomy,
        now: now.to_string(),
    };
    Built {
        request: Some(request),
        inputs: Some(inputs),
        gaps,
        verdict: Verdict::Evaluate,
    }
}

fn gap(input: GapInput, reason: GapReason) -> InputGap {
    InputGap { input, reason }
}

/// `name:<byte_len>:<value>\n` per argument, in name order.
fn canonical_args(candidate: &ShadowCandidate) -> CanonicalArgs {
    let mut text = String::new();
    for (name, value) in &candidate.args {
        text.push_str(&format!("{name}:{}:{value}\n", value.len()));
    }
    CanonicalArgs::from_canonical(text)
}

/// The identity of every distinct pinned host that resolves, and, for a unit contract, of every
/// pinned unit whose canonical identity can be built. Container subjects are not resolved here:
/// the sweep makes no live read.
async fn pinned_identities(
    candidate: &ShadowCandidate,
    env: &dyn ShadowEnvironment,
    units: bool,
    gaps: &mut Vec<InputGap>,
) -> BTreeSet<domovoi_core::capability::TargetIdentity> {
    let hosts: BTreeSet<HostId> = candidate.pinned.iter().map(|p| p.host).collect();
    let mut identities = BTreeSet::new();
    for host in hosts {
        match env.host_target(host).await {
            Ok(target) => {
                identities.insert(target.identity);
            }
            Err(_) => gaps.push(gap(GapInput::PinnedTargets, GapReason::Unresolvable)),
        }
    }
    let mut unresolved = false;
    for subject in candidate.pinned.iter().filter(|p| p.target.is_some()) {
        let unit = match (&subject.target, units) {
            (Some(key), true) => env.unit_identity(subject.host, key.as_str()).await,
            _ => None,
        };
        match unit {
            Some(identity) => {
                identities.insert(identity);
            }
            None => unresolved = true,
        }
    }
    if unresolved {
        gaps.push(gap(GapInput::PinnedTargets, GapReason::NotProducedYet));
    }
    identities
}

/// `{host name -> host identity}` when a rule names the host, otherwise nothing.
fn rule_targets(
    rules: &[Rule],
    host_target: &HostTarget,
) -> BTreeMap<TargetName, domovoi_core::capability::TargetIdentity> {
    if names_to_resolve(rules).contains(&host_target.name) {
        BTreeMap::from([(host_target.name.clone(), host_target.identity.clone())])
    } else {
        BTreeMap::new()
    }
}

/// The contract's declared facts plus every fact a rule requires.
fn wanted_facts(declared: &[FactId], rules: &[Rule]) -> BTreeSet<FactId> {
    let mut wanted: BTreeSet<FactId> = declared.iter().cloned().collect();
    for rule in rules {
        for condition in &rule.conditions {
            if let Condition::RequiresFact { fact, .. } = condition {
                wanted.insert(fact.clone());
            }
        }
    }
    wanted
}
