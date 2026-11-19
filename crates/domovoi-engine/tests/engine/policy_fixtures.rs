//! Shared builders for the policy evaluator tests: a real contract index, requests that an
//! ordinary `Allow` rule satisfies, and a small rule builder.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::HostId;
use domovoi_core::capability::{
    CapabilityContract, CeilingEntry, CeilingSummary, CeilingTargets, ConstrainableArg,
    ConstraintKind, ContractCategory, ContractId, ContractIndex, ContractSource, ContractVersion,
    EffectClass, ExecutionPath, FactId, Idempotency, Reversibility, TargetIdentity, TargetKind,
    TargetName, VerificationHint,
};
use domovoi_engine::policy::{
    AutonomyState, CanonicalArgs, CapabilityScope, Condition, ContainmentInput, Decision,
    EvaluateRequest, HostScope, Outcome, Rule, RuleId, TargetScope, evaluate,
};
use jiff::{SignedDuration, Timestamp};

/// A mutating container contract with a constrainable argument.
pub const RESTART: &str = "container.restart";
/// A mutating contract in the same category as [`RESTART`] that declares a safety fact.
pub const GATED: &str = "container.stop";
/// A read contract.
pub const READ: &str = "container.list";
/// A containment contract.
pub const CONTAIN: &str = "network.block";
/// A destructive contract.
pub const DESTROY: &str = "container.remove";
/// A destructive contract that declares a safety fact.
pub const PURGE: &str = "container.purge";
/// The category of [`RESTART`] and [`GATED`].
pub const LIFECYCLE: &str = "container.lifecycle";
/// The safety fact [`GATED`] and [`PURGE`] declare.
pub const FACT: &str = "host.not_in_maintenance";

pub fn cid(text: &str) -> ContractId {
    ContractId::parse(text).unwrap()
}

pub fn cat(text: &str) -> ContractCategory {
    ContractCategory::parse(text).unwrap()
}

pub fn name(text: &str) -> TargetName {
    TargetName::parse(text).unwrap()
}

pub fn names(texts: &[&str]) -> BTreeSet<TargetName> {
    texts.iter().map(|t| name(t)).collect()
}

pub fn ident(text: &str) -> TargetIdentity {
    TargetIdentity::parse(text).unwrap()
}

pub fn fact(text: &str) -> FactId {
    FactId::parse(text).unwrap()
}

pub fn rid(text: &str) -> RuleId {
    RuleId::parse(text).unwrap()
}

pub fn ids(texts: &[&str]) -> Vec<RuleId> {
    texts.iter().map(|t| rid(t)).collect()
}

/// A timestamp on 1970-01-01 UTC.
pub fn ts(hour: i64, minute: i64) -> Timestamp {
    Timestamp::from_second(hour * 3600 + minute * 60).unwrap()
}

fn contract(
    id: &str,
    category: &str,
    effect: EffectClass,
    kind: TargetKind,
    facts: &[&str],
) -> CapabilityContract {
    let read = effect == EffectClass::Read;
    CapabilityContract {
        id: cid(id),
        version: ContractVersion::new(1).unwrap(),
        title: "Test contract".to_owned(),
        description: "A contract built for a test.".to_owned(),
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
        effect,
        category: cat(category),
        target_kind: kind,
        target_arg: matches!(kind, TargetKind::Container | TargetKind::Unit)
            .then(|| "container".to_owned()),
        reversibility: if read {
            Reversibility::NotApplicable
        } else {
            Reversibility::Reversible
        },
        idempotency: if read {
            Idempotency::Idempotent
        } else {
            Idempotency::NotIdempotent
        },
        timeout: SignedDuration::from_secs(15),
        output_limit_bytes: 4096,
        verification: if read {
            VerificationHint::None
        } else {
            VerificationHint::Recheck
        },
        constrainable_args: Vec::new(),
        safety_facts: facts.iter().map(|f| fact(f)).collect(),
        execution: ExecutionPath::Agent,
        source: ContractSource::BuiltIn,
    }
}

/// The index every policy test shares.
pub fn index() -> ContractIndex {
    use EffectClass::{Contain, Destructive, Mutate, Read};
    use TargetKind::{Container, Host};
    let mut restart = contract(RESTART, LIFECYCLE, Mutate, Container, &[]);
    restart.constrainable_args = vec![ConstrainableArg {
        key: "wait_seconds".to_owned(),
        kind: ConstraintKind::IntAtMost,
        description: "How long to wait before the restart.".to_owned(),
    }];
    ContractIndex::new(vec![
        restart,
        contract(GATED, LIFECYCLE, Mutate, Container, &[FACT]),
        contract(READ, "container.observe", Read, Host, &[]),
        contract(CONTAIN, "network.containment", Contain, Host, &[]),
        contract(DESTROY, "container.destroy", Destructive, Container, &[]),
        contract(PURGE, "container.destroy", Destructive, Container, &[FACT]),
    ])
    .unwrap()
}

/// Two hosts, the contract index and request builders.
pub struct Fx {
    pub index: ContractIndex,
    pub host: HostId,
    pub other: HostId,
}

impl Fx {
    pub fn new() -> Self {
        Self {
            index: index(),
            host: HostId::new(),
            other: HostId::new(),
        }
    }

    /// A ceiling that covers every contract, any target, ten runs an hour.
    pub fn ceiling(&self) -> CeilingSummary {
        CeilingSummary {
            enabled: true,
            entries: self
                .index
                .iter()
                .map(|c| CeilingEntry {
                    contract: c.id.clone(),
                    targets: CeilingTargets::Any,
                    max_per_hour: 10,
                    max_active: None,
                    max_ttl: None,
                    args: BTreeMap::new(),
                })
                .collect(),
            hash: Some("h1".to_owned()),
        }
    }

    /// A request for `contract` on `plex` that a plain `Allow` rule authorizes.
    pub fn req(&self, contract: &str) -> EvaluateRequest {
        let meta = self.index.meta(&cid(contract)).unwrap();
        let identity = ident("container:abc/plex");
        let containment = if meta.effect == EffectClass::Contain {
            ContainmentInput {
                attested: true,
                ..ContainmentInput::default()
            }
        } else {
            ContainmentInput::not_applicable()
        };
        EvaluateRequest {
            host: self.host,
            contract: meta,
            contract_version: ContractVersion::new(1).unwrap(),
            target: name("plex"),
            target_identity: identity.clone(),
            args: CanonicalArgs::from_canonical(""),
            requested_rate_per_hour: None,
            requested_ttl: None,
            pinned: BTreeSet::from([identity]),
            rule_targets: BTreeMap::new(),
            ceiling: Some(self.ceiling()),
            expected_ceiling_hash: None,
            facts: BTreeMap::new(),
            runs_as_root: false,
            containment,
            autonomy: AutonomyState::default(),
            now: ts(12, 0),
            policy_version: None,
        }
    }
}

/// An enabled, enforced, unlocked rule over everything, with no conditions.
pub fn rule(id: &str, outcome: Outcome) -> Rule {
    Rule {
        id: rid(id),
        enabled: true,
        shadow: false,
        locked: false,
        host: HostScope::All,
        capability: CapabilityScope::Any,
        targets: TargetScope::Any,
        conditions: Vec::new(),
        outcome,
        note: String::new(),
        version_id: None,
    }
}

/// Chainable edits for a [`Rule`].
pub trait RuleExt {
    fn on_host(self, host: HostId) -> Self;
    fn contract(self, id: &str) -> Self;
    fn category(self, category: &str) -> Self;
    fn effect(self, effect: EffectClass) -> Self;
    fn named(self, targets: &[&str]) -> Self;
    fn locked(self) -> Self;
    fn shadow(self) -> Self;
    fn off(self) -> Self;
    fn when(self, condition: Condition) -> Self;
}

impl RuleExt for Rule {
    fn on_host(mut self, host: HostId) -> Self {
        self.host = HostScope::Host(host);
        self
    }

    fn contract(mut self, id: &str) -> Self {
        self.capability = CapabilityScope::Contract(cid(id));
        self
    }

    fn category(mut self, category: &str) -> Self {
        self.capability = CapabilityScope::Category(cat(category));
        self
    }

    fn effect(mut self, effect: EffectClass) -> Self {
        self.capability = CapabilityScope::Effect(effect);
        self
    }

    fn named(mut self, targets: &[&str]) -> Self {
        self.targets = TargetScope::Exact(names(targets));
        self
    }

    fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    fn shadow(mut self) -> Self {
        self.shadow = true;
        self
    }

    fn off(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn when(mut self, condition: Condition) -> Self {
        self.conditions.push(condition);
        self
    }
}

/// Evaluates and unwraps: the request and the rules in these tests are well formed.
pub fn eval(rules: &[Rule], request: &EvaluateRequest) -> Decision {
    evaluate(rules, request).unwrap()
}
