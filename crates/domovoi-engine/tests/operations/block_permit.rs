//! A block of a network source authorized by a policy permit, through the operation service: the
//! permit names the verified helper identity, the three phases share one plan hash, and nothing is
//! sent when the identity, the drills or the permit itself do not hold. The permit issuer, the
//! relay and the helper's trust are scripted; no test touches a host.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::action::PlanAssurance;
use domovoi_core::attest::{Authority, ExpectedSafety};
use domovoi_core::capability::{Availability, ContractId};
use domovoi_core::containment::{ContainCall, ContainOutputs, keys};
use domovoi_core::ports::{
    LinkError, NotIssued, PermitGrant, PermitIssuer, PermitOutcome, PermitRefusal, PermitRequest,
};
use domovoi_core::{
    Actor, Authorization, AuthorizationKind, CheckKey, ExecutionIntent, HostId, OperationId,
    OperationKind, PinnedSubject, PolicyPermit, PolicyPermitParts, StepId, StepOutcome, TargetKey,
};
use domovoi_engine::containment::input::assess;
use domovoi_engine::containment::{ContainDeps, ContainmentDriver, PathSource};
use domovoi_engine::operations::{AutonomyDeps, ExecutionRequest, OperationSeed};
use domovoi_testkit::build::T0;
use domovoi_testkit::contain::contain_caps;
use domovoi_testkit::helper_attest::{FakeHelperTrust, TestHelper, verified_view};
use domovoi_testkit::{FakeContainRelay, FakeTargetResolver, FakeUnitRelay};
use jiff::SignedDuration;

use crate::harness::{World, host_id};

const SOURCE: &str = "203.0.113.0/24";
const TTL_S: i64 = 600;

/// No management path is discovered: a verified report covers an empty list.
struct NoPaths;

#[async_trait]
impl PathSource for NoPaths {
    async fn discovered(&self, _host: HostId) -> Result<Vec<domovoi_core::attest::Cidr>, String> {
        Ok(Vec::new())
    }
}

/// What the scripted issuer hands out.
enum Permit {
    /// A permit naming this verified identity, or none.
    Identity(Option<String>),
    /// No permit: a person decides.
    Withheld,
}

struct Issuer {
    permit: Permit,
}

#[async_trait]
impl PermitIssuer for Issuer {
    async fn issue(&self, request: PermitRequest) -> PermitOutcome {
        let Permit::Identity(identity) = &self.permit else {
            return PermitOutcome::NotIssued(NotIssued::Withheld(PermitRefusal::Paused));
        };
        let issued = request.execution.created_at;
        let permit = PolicyPermit::issue(PolicyPermitParts {
            decision_id: "decision-1",
            rule_ids: vec!["rule-block".to_owned()],
            policy_version: "4",
            plan_hash: request.execution.plan_hash,
            issued_at: issued,
            expires_at: issued + SignedDuration::from_mins(5),
            ceiling_hash: &"cd".repeat(32),
            safety_identity: identity.as_deref(),
        })
        .unwrap();
        PermitOutcome::Issued(PermitGrant {
            authorization: Authorization::policy(permit),
            decision: "{\"outcome\":\"allow\"}".to_owned(),
        })
    }
}

struct Rig {
    w: World,
    relay: Arc<FakeContainRelay>,
    driver: Arc<ContainmentDriver>,
    /// The identity the verified helper has now, as a permit names it.
    identity: String,
}

/// The canonical text of the identity a verified helper has.
async fn verified_identity(trust: &Arc<FakeHelperTrust>) -> String {
    let assessment = assess(&**trust, &NoPaths, host_id(), Authority::Policy, T0)
        .await
        .unwrap();
    let PlanAssurance::Verified(identity) = assessment.assurance else {
        panic!("the helper was verified");
    };
    identity.canonical()
}

/// A world whose service holds a permit issuer of `permit` and a containment driver over a
/// verified helper. `identity` overrides what the permit names (the helper's own by default).
async fn rig(permit: impl FnOnce(&str) -> Permit) -> Rig {
    let store = Arc::new(domovoi_testkit::InMemoryStore::new());
    let trust = Arc::new(FakeHelperTrust::new());
    trust.set(
        host_id(),
        verified_view(&TestHelper::new(7), 1, 5, T0).unwrap(),
    );
    let identity = verified_identity(&trust).await;
    let relay = Arc::new(FakeContainRelay::new());
    let autonomy = AutonomyDeps {
        relay: Arc::new(FakeUnitRelay::new()),
        permits: store.clone(),
        issuer: Arc::new(Issuer {
            permit: permit(&identity),
        }),
    };
    let built = (trust.clone(), relay.clone());
    let w = crate::harness::world_over_with_autonomy_and_contain(
        store,
        autonomy,
        Box::new(move |link, store, clock| {
            ContainmentDriver::new(ContainDeps {
                caps: contain_caps(link.clone(), built.1.clone()),
                uow: store.clone(),
                blocks: store.clone(),
                trust: built.0.clone(),
                paths: Arc::new(NoPaths),
                resolver: Arc::new(FakeTargetResolver::new()),
                clock: clock.clone(),
                result_grace: SignedDuration::from_secs(30),
            })
        }),
    )
    .await;
    w.registry.set_availability(
        host_id(),
        &ContractId::parse("firewall.block_source").unwrap(),
        Availability::Available,
    );
    let driver = w.contain.clone().expect("the world holds a driver");
    Rig {
        w,
        relay,
        driver,
        identity,
    }
}

fn outputs(pairs: &[(&str, &str)]) -> Result<ContainOutputs, LinkError> {
    Ok(ContainOutputs(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    ))
}

/// The host's last refresh says the drills passed, as an owner's refresh would.
async fn drills_passed(r: &Rig) {
    r.relay.queue_result(outputs(&[
        (keys::OUT_GATE, "open"),
        (keys::OUT_DRILLS, "passed"),
    ]));
    r.driver
        .refresh(host_id(), Actor::System("tests".to_owned()))
        .await
        .expect("the refresh is answered");
}

/// What was sent to the agent, without the owner's refresh.
fn phases(r: &Rig) -> Vec<domovoi_core::containment::ContainDispatch> {
    r.relay
        .dispatches()
        .into_iter()
        .filter(|dispatch| !matches!(dispatch.call(), ContainCall::List))
        .collect()
}

fn key() -> TargetKey {
    TargetKey::parse(&format!("ipnet:{SOURCE}")).unwrap()
}

async fn propose(r: &Rig) -> (OperationId, StepId) {
    let seed = OperationSeed {
        kind: OperationKind::Investigate,
        goal: "a host keeps hammering the login".to_owned(),
        pinned: vec![PinnedSubject {
            host: host_id(),
            check: CheckKey::parse("firewall.block").unwrap(),
            target: Some(key()),
        }],
        incidents: Vec::new(),
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: Actor::System("tests".to_owned()),
    };
    let op = r.w.handle().create(seed).await.unwrap();
    let request = ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse("firewall.block_source").unwrap(),
            contract_version: 1,
            host: host_id(),
            check: CheckKey::parse("firewall.block").unwrap(),
            target: Some(key()),
            args: BTreeMap::from([
                (keys::ARG_SOURCE.to_owned(), SOURCE.to_owned()),
                (keys::ARG_TTL_S.to_owned(), TTL_S.to_string()),
            ]),
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    };
    let step = r.w.handle().propose_execution(op, request).await.unwrap();
    (op, step)
}

async fn reason(w: &World, op: OperationId, step: StepId) -> String {
    w.load(op)
        .await
        .step(step)
        .unwrap()
        .progress
        .reason
        .clone()
        .unwrap_or_default()
}

#[tokio::test]
async fn a_policy_permit_authorizes_and_runs_a_block_on_a_verified_helper() {
    let r = rig(|identity| Permit::Identity(Some(identity.to_owned()))).await;
    drills_passed(&r).await;
    r.relay
        .queue_result(outputs(&[(keys::OUT_EXISTING_FLOWS, "n/a")]));
    r.relay
        .queue_result(outputs(&[(keys::OUT_BRIDGE_GAP, "false")]));
    let (op, step) = propose(&r).await;
    let snapshot = r.w.until_outcome(op, step, StepOutcome::Succeeded).await;

    // Authorized by the permit, which names the verified identity; nobody approved.
    let row = &snapshot.execution_of(step).unwrap().value;
    assert_eq!(row.authorization_kind(), Some(AuthorizationKind::Policy));
    assert!(row.approval().is_none());
    assert_eq!(
        row.permit().unwrap().permit().safety_identity(),
        Some(r.identity.as_str())
    );
    // Apply, then confirm: one plan hash, two execution ids, the policy label, the stored
    // verified identity at every phase and never an unverified one.
    let sent = phases(&r);
    assert_eq!(sent.len(), 2, "apply and confirm");
    assert!(matches!(sent[0].call(), ContainCall::Apply(_)));
    assert!(matches!(sent[1].call(), ContainCall::Confirm(_)));
    assert_eq!(sent[0].plan_hash(), row.plan_hash);
    assert_eq!(sent[1].plan_hash(), row.plan_hash);
    assert_ne!(sent[0].execution(), sent[1].execution());
    for dispatch in &sent {
        assert_eq!(dispatch.authorization(), AuthorizationKind::Policy);
        assert!(matches!(
            dispatch.expected_safety(),
            Some(ExpectedSafety::Verified { .. })
        ));
    }
    // The owner's release is the third phase, under the same plan hash.
    r.relay.queue_result(outputs(&[]));
    let block = sent[0].execution();
    r.driver
        .release(block)
        .await
        .expect("the block is released");
    let released = phases(&r);
    assert_eq!(released.len(), 3);
    assert!(matches!(released[2].call(), ContainCall::Rollback(_)));
    assert_eq!(released[2].plan_hash(), row.plan_hash);
    let audit = r.w.audit_kinds().await;
    assert!(
        audit.contains(&"policy.permit_issued".to_owned()),
        "{audit:?}"
    );
}

#[tokio::test]
async fn a_permit_for_another_identity_than_the_helper_has_now_sends_nothing() {
    let r = rig(|_| Permit::Identity(Some(format!("v1:{}", "0".repeat(8))))).await;
    drills_passed(&r).await;
    let (op, step) = propose(&r).await;
    r.w.until_outcome(op, step, StepOutcome::Refused).await;
    assert!(phases(&r).is_empty());
    assert_eq!(reason(&r.w, op, step).await, "safety_changed");
}

#[tokio::test]
async fn a_permit_is_not_enough_without_passed_drills() {
    let r = rig(|identity| Permit::Identity(Some(identity.to_owned()))).await;
    let (op, step) = propose(&r).await;
    r.w.until_outcome(op, step, StepOutcome::Refused).await;
    assert!(phases(&r).is_empty());
    assert_eq!(reason(&r.w, op, step).await, "drills_not_passed");
}

#[tokio::test]
async fn a_permit_that_names_no_identity_never_authorizes_a_block() {
    let r = rig(|_| Permit::Identity(None)).await;
    drills_passed(&r).await;
    let (op, step) = propose(&r).await;
    r.w.settle().await;
    // Nobody authorized it: the approval offer stands and nothing was sent.
    assert_eq!(r.w.outcome(op, step).await, StepOutcome::Pending);
    let snapshot = r.w.load(op).await;
    assert!(!snapshot.execution_of(step).unwrap().value.is_authorized());
    assert!(phases(&r).is_empty());
}

#[tokio::test]
async fn a_withheld_permit_leaves_a_block_to_a_person() {
    let r = rig(|_| Permit::Withheld).await;
    drills_passed(&r).await;
    let (op, step) = propose(&r).await;
    r.w.settle().await;
    assert_eq!(r.w.outcome(op, step).await, StepOutcome::Pending);
    assert!(phases(&r).is_empty());
}
