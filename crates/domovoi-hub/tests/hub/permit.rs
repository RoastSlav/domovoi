//! The permit issuer: a permit exists only for an enforced `Allow` that held every guard, and the
//! reason a permit is withheld is exact.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::capability::{
    CeilingEntry, CeilingSummary, CeilingTargets, ContractId, ContractIndex, ContractMeta, FactId,
    TargetName, builtin_contracts, unit_contracts,
};
use domovoi_core::policy_store::{DraftGuard, PolicyWrite};
use domovoi_core::ports::{
    AuditFilter, AuditLog, NotIssued, Page, PermitIssuer, PermitOutcome, PermitRefusal,
    PermitRequest, PermitWrite, RateScope, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::unit::UnitRefusal;
use domovoi_core::{
    Actor, CapabilityPlan, CheckKey, ExecutionId, ExecutionIntent, HostId, OperationId,
    PinnedSubject, PolicyVersionId, Risk, StepId, TargetKey,
};
use domovoi_engine::policy::{
    CapabilityScope, FactState, HostScope, Outcome, Rule, RuleId, TargetScope,
};
use domovoi_engine::policy_shadow::{
    AutonomyFacts, AutonomyReading, CeilingReading, ContainmentReading, GapReason, HostTarget,
    ShadowEnvironment, rules_hash, stored_rules,
};
use domovoi_hub::policy::{HubPermitIssuer, IssuerDeps, PermitSettings};
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::{FakeClock, FakeTargetResolver, InMemoryStore};
use jiff::SignedDuration;

const UNIT: &str = "example.service";

#[cfg(test)]
struct Env {
    resolver: Arc<FakeTargetResolver>,
    facts: Mutex<AutonomyFacts>,
    ceiling: Mutex<Option<CeilingSummary>>,
}

#[cfg(test)]
fn index() -> ContractIndex {
    let mut all = builtin_contracts();
    all.extend(unit_contracts());
    ContractIndex::new(all).unwrap()
}

#[async_trait]
impl ShadowEnvironment for Env {
    async fn contract(&self, _host: HostId, id: &ContractId) -> Option<ContractMeta> {
        index().meta(id)
    }
    async fn host_target(&self, host: HostId) -> Result<HostTarget, GapReason> {
        use domovoi_core::capability::TargetQuery;
        use domovoi_core::ports::TargetResolver;
        let resolved = self
            .resolver
            .resolve(TargetQuery::Host { host })
            .await
            .map_err(|_| GapReason::Unresolvable)?;
        Ok(HostTarget {
            name: resolved.name,
            identity: resolved.identity,
        })
    }
    async fn unit_target(&self, host: HostId, typed: &TargetName) -> Result<HostTarget, GapReason> {
        use domovoi_core::ports::TargetResolver;
        let unit = self
            .resolver
            .resolve_unit(host, typed)
            .await
            .map_err(|_| GapReason::Unresolvable)?;
        Ok(HostTarget {
            name: typed.clone(),
            identity: unit.target.identity,
        })
    }
    async fn unit_identity(
        &self,
        host: HostId,
        unit: &str,
    ) -> Option<domovoi_core::capability::TargetIdentity> {
        use domovoi_core::ports::TargetResolver;
        let name = TargetName::parse(unit).ok()?;
        self.resolver
            .resolve_unit(host, &name)
            .await
            .ok()
            .map(|u| u.target.identity)
    }
    async fn ceiling(&self, _host: HostId) -> CeilingReading {
        match self.ceiling.lock().unwrap().clone() {
            Some(summary) => CeilingReading::Reported(summary),
            None => CeilingReading::Absent,
        }
    }
    async fn runs_as_root(&self, _host: HostId) -> Option<bool> {
        Some(false)
    }
    async fn facts(&self, _: HostId, _: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState> {
        BTreeMap::new()
    }
    async fn containment(&self, _host: HostId) -> ContainmentReading {
        ContainmentReading::NotApplicable
    }
    async fn autonomy(&self) -> AutonomyReading {
        AutonomyReading::Reported(self.facts.lock().unwrap().clone())
    }
    async fn publication_index(&self) -> ContractIndex {
        index()
    }
    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary> {
        BTreeMap::new()
    }
}

struct Rig {
    store: Arc<InMemoryStore>,
    issuer: HubPermitIssuer,
    resolver: Arc<FakeTargetResolver>,
    env: Arc<Env>,
    host: HostId,
}

fn allow_rule(id: &str, outcome: Outcome) -> Rule {
    Rule {
        id: RuleId::parse(id).unwrap(),
        enabled: true,
        shadow: false,
        locked: false,
        host: HostScope::All,
        capability: CapabilityScope::Contract(ContractId::parse("unit.restart").unwrap()),
        targets: TargetScope::Any,
        conditions: Vec::new(),
        outcome,
        note: String::new(),
        version_id: None,
    }
}

fn ceiling() -> CeilingSummary {
    CeilingSummary {
        enabled: true,
        entries: vec![CeilingEntry {
            contract: ContractId::parse("unit.restart").unwrap(),
            targets: CeilingTargets::Any,
            max_per_hour: 5,
            max_active: None,
            max_ttl: None,
            args: BTreeMap::new(),
        }],
        hash: Some("cd".repeat(32)),
    }
}

async fn publish(store: &Arc<InMemoryStore>, rules: &[Rule]) {
    let id = PolicyVersionId::new();
    let stored = stored_rules(rules);
    let hash = rules_hash(&stored);
    let actor = Actor::System("tests".to_owned());
    for write in [
        PolicyWrite::SaveDraft {
            id,
            expected: DraftGuard::Absent,
            based_on: None,
            rules: stored,
            rules_hash: hash,
            actor: actor.clone(),
            at: T0,
        },
        PolicyWrite::Publish {
            id,
            expected_revision: 1,
            number: 1,
            acknowledgements: Vec::new(),
            actor,
            at: T0,
        },
    ] {
        store
            .commit(Transaction::new().write(StateWrite::Policy(write)))
            .await
            .unwrap();
    }
}

async fn rig(rules: Option<Vec<Rule>>, max_per_hour: u32) -> Rig {
    let nas = host("nas").unwrap();
    let store = Arc::new(InMemoryStore::new());
    if let Some(rules) = rules {
        publish(&store, &rules).await;
    }
    let resolver = Arc::new(FakeTargetResolver::new());
    resolver.set_unit(nas.id, UNIT, UNIT, 'c', "untested");
    let env = Arc::new(Env {
        resolver: resolver.clone(),
        facts: Mutex::new(AutonomyFacts::default()),
        ceiling: Mutex::new(Some(ceiling())),
    });
    let issuer = HubPermitIssuer::new(IssuerDeps {
        policy: store.clone(),
        permits: store.clone(),
        resolver: resolver.clone(),
        env: env.clone(),
        uow: store.clone(),
        clock: Arc::new(FakeClock::new(T0)),
        settings: PermitSettings {
            ttl: SignedDuration::from_mins(5),
            max_per_hour,
        },
    });
    Rig {
        store,
        issuer,
        resolver,
        env,
        host: nas.id,
    }
}

/// A proposed `unit.restart` of `UNIT`, planned against what the resolver says now.
async fn request(r: &Rig, contract: &str) -> PermitRequest {
    use domovoi_core::ports::TargetResolver;
    let args = BTreeMap::from([("unit".to_owned(), UNIT.to_owned())]);
    let unit = r
        .resolver
        .resolve_unit(r.host, &TargetName::parse(UNIT).unwrap())
        .await
        .unwrap();
    let step = StepId::new();
    let plan = CapabilityPlan::unit_restart(
        r.host,
        &unit.target.host_key,
        &unit.target.identity,
        &unit.identity_hash,
        &args,
        ExecutionId::from_step(step),
    )
    .hash();
    let intent = ExecutionIntent {
        contract: ContractId::parse(contract).unwrap(),
        contract_version: 1,
        host: r.host,
        check: CheckKey::parse("unit.failed").unwrap(),
        target: Some(TargetKey::parse(UNIT).unwrap()),
        args,
        retry_of: None,
    };
    let execution = domovoi_core::ExecutionRecord::propose(
        &intent,
        step,
        OperationId::new(),
        plan,
        T0 + SignedDuration::from_hours(1),
        Risk::Write,
        T0,
    );
    PermitRequest {
        execution,
        pinned: vec![PinnedSubject {
            host: r.host,
            check: CheckKey::parse("unit.failed").unwrap(),
            target: Some(TargetKey::parse(UNIT).unwrap()),
        }],
    }
}

fn withheld(outcome: &PermitOutcome) -> &PermitRefusal {
    match outcome {
        PermitOutcome::NotIssued(NotIssued::Withheld(refusal)) => refusal,
        other => panic!("expected a withheld permit, got {other:?}"),
    }
}

#[tokio::test]
async fn an_enforced_allow_that_held_every_guard_is_issued_with_a_capped_expiry() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    let req = request(&r, "unit.restart").await;
    let PermitOutcome::Issued(grant) = r.issuer.issue(req.clone()).await else {
        panic!("no permit was issued");
    };
    let permit = grant
        .authorization
        .policy_permit()
        .expect("a policy permit");
    assert_eq!(permit.rule_ids(), ["r-restart"]);
    assert_eq!(permit.policy_version(), "1");
    assert_eq!(permit.plan_hash(), req.execution.plan_hash);
    assert_eq!(permit.expires_at(), T0 + SignedDuration::from_mins(5));
    assert!(grant.decision.contains("allow"), "{}", grant.decision);
}

#[tokio::test]
async fn permit_expiry_is_capped_by_the_execution_window() {
    let mut r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    // A TTL longer than the step's own window is cut to it.
    r.issuer = HubPermitIssuer::new(IssuerDeps {
        policy: r.store.clone(),
        permits: r.store.clone(),
        resolver: r.resolver.clone(),
        env: r.env.clone(),
        uow: r.store.clone(),
        clock: Arc::new(FakeClock::new(T0)),
        settings: PermitSettings {
            ttl: SignedDuration::from_hours(5),
            max_per_hour: 12,
        },
    });
    let req = request(&r, "unit.restart").await;
    let PermitOutcome::Issued(grant) = r.issuer.issue(req.clone()).await else {
        panic!("no permit was issued");
    };
    let permit = grant.authorization.policy_permit().unwrap();
    assert_eq!(permit.expires_at(), req.execution.expires_at);
}

#[tokio::test]
async fn no_permit_without_policy_allow() {
    // No published policy.
    let r = rig(None, 12).await;
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert_eq!(withheld(&outcome), &PermitRefusal::NoPublishedPolicy);
    // The effect-class default is to ask.
    let r = rig(Some(Vec::new()), 12).await;
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert!(
        matches!(
            outcome,
            PermitOutcome::NotIssued(NotIssued::Evaluated { ref outcome, .. }) if outcome == "require_approval"
        ),
        "{outcome:?}"
    );
    // A rule that says require_approval, and one that says deny.
    for rule_outcome in [Outcome::RequireApproval, Outcome::Deny] {
        let r = rig(Some(vec![allow_rule("r-restart", rule_outcome)]), 12).await;
        let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
        assert!(matches!(
            outcome,
            PermitOutcome::NotIssued(NotIssued::Evaluated { .. })
        ));
    }
    // A shadow-only Allow is never enforced.
    let mut shadow = allow_rule("r-shadow", Outcome::Allow);
    shadow.shadow = true;
    let r = rig(Some(vec![shadow]), 12).await;
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert!(matches!(
        outcome,
        PermitOutcome::NotIssued(NotIssued::Evaluated { .. })
    ));
}

#[tokio::test]
async fn ceiling_missing_or_not_covering_gives_approval_not_permit() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    *r.env.ceiling.lock().unwrap() = None;
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert!(
        matches!(
            outcome,
            PermitOutcome::NotIssued(NotIssued::Evaluated { ref outcome, .. }) if outcome == "require_approval"
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn skill_contracts_are_never_permitted() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    for contract in ["skill.restart_container", "container.restart"] {
        let outcome = r.issuer.issue(request(&r, contract).await).await;
        assert_eq!(
            withheld(&outcome),
            &PermitRefusal::NotPolicyDispatchable,
            "{contract}"
        );
    }
}

#[tokio::test]
async fn pause_blocks_new_permits_and_the_withholding_is_audited() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    r.store
        .commit(
            Transaction::new().write(StateWrite::Permit(PermitWrite::Pause {
                by: "owner".to_owned(),
                reason: "maintenance".to_owned(),
                at: T0,
            })),
        )
        .await
        .unwrap();
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert_eq!(withheld(&outcome), &PermitRefusal::Paused);
    let page = AuditLog::list(
        &*r.store,
        &AuditFilter {
            kind: Some("policy.permit_withheld".to_owned()),
            ..AuditFilter::default()
        },
        &Page::first(5),
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn a_suspended_rule_withholds_the_permit() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    r.env
        .facts
        .lock()
        .unwrap()
        .suspended_rules
        .insert("r-restart".to_owned());
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert_eq!(
        withheld(&outcome),
        &PermitRefusal::RuleSuspended("r-restart".to_owned())
    );
}

#[tokio::test]
async fn permit_rate_limit_holds_for_the_whole_hub() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 0).await;
    let outcome = r.issuer.issue(request(&r, "unit.restart").await).await;
    assert_eq!(
        withheld(&outcome),
        &PermitRefusal::RateLimited(RateScope::Global),
        "0 means no permit is ever issued"
    );
}

#[tokio::test]
async fn a_configuration_change_or_a_helper_refusal_since_the_proposal_withholds_the_permit() {
    let r = rig(Some(vec![allow_rule("r-restart", Outcome::Allow)]), 12).await;
    let req = request(&r, "unit.restart").await;
    // The unit changed since the plan was made.
    r.resolver.set_unit(r.host, UNIT, UNIT, 'd', "untested");
    let outcome = r.issuer.issue(req.clone()).await;
    assert_eq!(withheld(&outcome), &PermitRefusal::PlanChanged);
    // The helper now refuses the unit.
    r.resolver
        .refuse_unit(r.host, UNIT, UnitRefusal::TargetStale);
    let outcome = r.issuer.issue(req).await;
    assert_eq!(
        withheld(&outcome),
        &PermitRefusal::IdentityRefused(UnitRefusal::TargetStale)
    );
}
