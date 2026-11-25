//! A unit restart through the operation service: authorized by a person or by a policy permit,
//! planned with the helper's identity, dispatched through the relay, and settled by what the agent
//! answers. The permit issuer, the relay and the target resolver are scripted; no test touches a
//! host.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::capability::{Availability, ContractId};
use domovoi_core::ports::{
    LinkError, NotIssued, PermitGrant, PermitIssuer, PermitOutcome, PermitRefusal, PermitRequest,
    PermitWrite, StateWrite, Transaction,
};
use domovoi_core::unit::{UnitCall, UnitOutputs};
use domovoi_core::{
    Actor, Authorization, AuthorizationKind, CheckKey, ExecutionIntent, OperationId, OperationKind,
    PinnedSubject, PolicyPermit, PolicyPermitParts, StepId, StepKind, StepOutcome, TargetKey,
};
use domovoi_engine::operations::{AutonomyDeps, ExecutionRequest, OperationSeed};
use domovoi_testkit::{FakeUnitRelay, identity_outputs};
use jiff::SignedDuration;

use crate::harness::{World, host_id, restart};

const UNIT: &str = "example.service";

/// What the scripted issuer answers.
enum Mode {
    Allow,
    Withhold(PermitRefusal),
    Evaluated,
}

struct Issuer {
    mode: Mutex<Mode>,
    asked: Mutex<u32>,
}

impl Issuer {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            asked: Mutex::new(0),
        })
    }

    fn asked(&self) -> u32 {
        *self.asked.lock().unwrap()
    }
}

#[async_trait]
impl PermitIssuer for Issuer {
    async fn issue(&self, request: PermitRequest) -> PermitOutcome {
        *self.asked.lock().unwrap() += 1;
        match &*self.mode.lock().unwrap() {
            Mode::Allow => {
                let issued = request.execution.created_at;
                let permit = PolicyPermit::issue(PolicyPermitParts {
                    decision_id: "decision-1",
                    rule_ids: vec!["rule-a".to_owned()],
                    policy_version: "4",
                    plan_hash: request.execution.plan_hash,
                    issued_at: issued,
                    expires_at: issued + SignedDuration::from_mins(5),
                    ceiling_hash: &"cd".repeat(32),
                    safety_identity: None,
                })
                .unwrap();
                PermitOutcome::Issued(PermitGrant {
                    authorization: Authorization::policy(permit),
                    decision: "{\"outcome\":\"allow\"}".to_owned(),
                })
            }
            Mode::Withhold(refusal) => {
                PermitOutcome::NotIssued(NotIssued::Withheld(refusal.clone()))
            }
            Mode::Evaluated => PermitOutcome::NotIssued(NotIssued::Evaluated {
                outcome: "require_approval".to_owned(),
                decision: "{}".to_owned(),
            }),
        }
    }
}

struct Rig {
    w: World,
    relay: Arc<FakeUnitRelay>,
    issuer: Arc<Issuer>,
}

fn deps(
    w_store: &Arc<domovoi_testkit::InMemoryStore>,
    relay: &Arc<FakeUnitRelay>,
    issuer: &Arc<Issuer>,
) -> AutonomyDeps {
    AutonomyDeps {
        relay: relay.clone(),
        permits: w_store.clone(),
        issuer: issuer.clone(),
    }
}

async fn rig(mode: Mode, gated: bool) -> Rig {
    // The autonomy deps need the world's store, which exists only once the world does, so the
    // world is built over a store made here.
    let store = Arc::new(domovoi_testkit::InMemoryStore::new());
    let relay = Arc::new(FakeUnitRelay::new());
    let issuer = Issuer::new(mode);
    let autonomy = deps(&store, &relay, &issuer);
    let w = if gated {
        world_gated_with_autonomy_over(store, autonomy).await
    } else {
        world_with_autonomy_over(store, autonomy).await
    };
    w.resolver.set_unit(host_id(), UNIT, UNIT, 'c', "untested");
    w.registry.set_availability(
        host_id(),
        &ContractId::parse("unit.restart").unwrap(),
        Availability::Available,
    );
    relay.set_identity(UNIT, identity_outputs(UNIT, 'c', "untested", true));
    Rig { w, relay, issuer }
}

async fn world_with_autonomy_over(
    store: Arc<domovoi_testkit::InMemoryStore>,
    autonomy: AutonomyDeps,
) -> World {
    crate::harness::world_over_with_autonomy(store, autonomy, false).await
}

async fn world_gated_with_autonomy_over(
    store: Arc<domovoi_testkit::InMemoryStore>,
    autonomy: AutonomyDeps,
) -> World {
    crate::harness::world_over_with_autonomy(store, autonomy, true).await
}

fn subject() -> PinnedSubject {
    PinnedSubject {
        host: host_id(),
        check: CheckKey::parse("unit.failed").unwrap(),
        target: Some(TargetKey::parse(UNIT).unwrap()),
    }
}

fn unit_seed() -> OperationSeed {
    OperationSeed {
        kind: OperationKind::Investigate,
        goal: "the service is failing".to_owned(),
        pinned: vec![subject()],
        incidents: Vec::new(),
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: Actor::System("tests".to_owned()),
    }
}

fn unit_restart() -> ExecutionRequest {
    ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse("unit.restart").unwrap(),
            contract_version: 1,
            host: host_id(),
            check: CheckKey::parse("unit.failed").unwrap(),
            target: Some(TargetKey::parse(UNIT).unwrap()),
            args: BTreeMap::from([("unit".to_owned(), UNIT.to_owned())]),
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    }
}

async fn propose(r: &Rig) -> (OperationId, StepId) {
    let op = r.w.handle().create(unit_seed()).await.unwrap();
    let step =
        r.w.handle()
            .propose_execution(op, unit_restart())
            .await
            .unwrap();
    (op, step)
}

fn verified() -> Result<UnitOutputs, LinkError> {
    Ok(UnitOutputs(BTreeMap::from([
        ("restarted".to_owned(), "true".to_owned()),
        ("verification".to_owned(), "verified".to_owned()),
        ("invocation_before".to_owned(), "aa".to_owned()),
        ("invocation_after".to_owned(), "bb".to_owned()),
    ])))
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
async fn a_policy_permit_authorizes_and_dispatches_a_unit_restart() {
    let r = rig(Mode::Allow, false).await;
    r.relay.queue_result(verified());
    let (op, step) = propose(&r).await;
    let snapshot = r.w.until_outcome(op, step, StepOutcome::Succeeded).await;

    // Authorized by the permit, not by a person.
    let row = &snapshot.execution_of(step).unwrap().value;
    assert_eq!(row.authorization_kind(), Some(AuthorizationKind::Policy));
    assert!(row.approval().is_none());
    assert_eq!(row.permit().unwrap().decision_id(), "decision-1");
    // The approval step was skipped, and a policy-decision step says why.
    let approval = snapshot
        .steps
        .iter()
        .find(|s| s.step.kind == StepKind::Approval)
        .unwrap();
    assert_eq!(approval.progress.outcome, StepOutcome::Skipped);
    assert_eq!(
        approval.progress.reason.as_deref(),
        Some("authorized by policy")
    );
    let decision = snapshot
        .steps
        .iter()
        .find(|s| s.step.kind == StepKind::PolicyDecision)
        .unwrap();
    assert_eq!(decision.progress.outcome, StepOutcome::Succeeded);
    assert!(
        decision
            .progress
            .summary
            .as_deref()
            .unwrap()
            .contains("rule-a")
    );
    // One restart was handed off, labelled policy, bound to the identity the helper reported.
    let sent = r.relay.dispatches();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].authorization_kind, AuthorizationKind::Policy);
    let UnitCall::Restart { id, identity_hash } = &sent[0].call else {
        panic!("a restart was sent");
    };
    assert_eq!(id.as_str(), UNIT);
    assert_eq!(identity_hash.to_string(), "c".repeat(64));
    let audit = r.w.audit_kinds().await;
    assert!(
        audit.contains(&"policy.permit_issued".to_owned()),
        "{audit:?}"
    );
    assert!(audit.contains(&"execution.dispatched".to_owned()));
    // The restart's plan hash is the capability family's, not a skill's.
    assert_eq!(row.contract.as_str(), "unit.restart");
}

#[tokio::test]
async fn human_approval_of_unit_restart_dispatches_with_human_label() {
    let r = rig(Mode::Evaluated, false).await;
    r.relay.queue_result(verified());
    let (op, step) = propose(&r).await;
    r.w.settle().await;
    // Nothing was issued: the approval offer stands and nothing was sent.
    assert_eq!(r.issuer.asked(), 1);
    assert_eq!(r.w.outcome(op, step).await, StepOutcome::Pending);
    assert!(r.relay.dispatches().is_empty());
    r.w.approve(op, step).await.unwrap();
    let snapshot = r.w.until_outcome(op, step, StepOutcome::Succeeded).await;
    let row = &snapshot.execution_of(step).unwrap().value;
    assert_eq!(row.authorization_kind(), Some(AuthorizationKind::Human));
    assert!(row.permit().is_none());
    assert_eq!(
        r.relay.dispatches()[0].authorization_kind,
        AuthorizationKind::Human
    );
}

#[tokio::test]
async fn no_permit_is_stored_unless_the_issuer_issues_one() {
    for mode in [
        Mode::Evaluated,
        Mode::Withhold(PermitRefusal::Paused),
        Mode::Withhold(PermitRefusal::NoPublishedPolicy),
    ] {
        let r = rig(mode, false).await;
        let (op, step) = propose(&r).await;
        r.w.settle().await;
        let snapshot = r.w.load(op).await;
        assert!(
            snapshot
                .execution_of(step)
                .unwrap()
                .value
                .permit()
                .is_none()
        );
        assert_eq!(
            snapshot.step(step).unwrap().progress.outcome,
            StepOutcome::Pending
        );
        assert!(r.relay.dispatches().is_empty());
    }
}

#[tokio::test]
async fn a_skill_proposal_is_never_offered_to_the_permit_issuer() {
    let r = rig(Mode::Allow, false).await;
    let op = r.w.create().await;
    let step =
        r.w.handle()
            .propose_execution(op, restart("plex"))
            .await
            .unwrap();
    r.w.settle().await;
    assert_eq!(
        r.issuer.asked(),
        0,
        "only policy-dispatchable contracts are offered"
    );
    assert_eq!(r.w.outcome(op, step).await, StepOutcome::Pending);
}

#[tokio::test]
async fn a_person_approving_first_wins() {
    let r = rig(Mode::Allow, true).await;
    r.relay.queue_result(verified());
    let (op, step) = propose(&r).await;
    // The person acts before the issuer is consulted.
    r.w.approve(op, step).await.unwrap();
    r.w.queue.open();
    let snapshot = r.w.until_outcome(op, step, StepOutcome::Succeeded).await;
    let row = &snapshot.execution_of(step).unwrap().value;
    assert_eq!(row.authorization_kind(), Some(AuthorizationKind::Human));
    assert!(
        row.permit().is_none(),
        "no second authorization was recorded"
    );
    assert_eq!(
        r.issuer.asked(),
        0,
        "an authorized step is not offered again"
    );
    assert_eq!(r.relay.dispatches().len(), 1);
}

#[tokio::test]
async fn unit_config_change_invalidates_approval() {
    let r = rig(Mode::Evaluated, false).await;
    let (op, step) = propose(&r).await;
    // The unit's configuration changes after the plan was made: a new identity hash.
    r.w.resolver
        .set_unit(host_id(), UNIT, UNIT, 'd', "untested");
    r.w.approve(op, step).await.unwrap();
    r.w.until_outcome(op, step, StepOutcome::Refused).await;
    assert!(r.relay.dispatches().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_unit_the_helper_now_refuses_is_refused_with_its_token() {
    let r = rig(Mode::Evaluated, false).await;
    let (op, step) = propose(&r).await;
    r.w.resolver.refuse_unit(
        host_id(),
        UNIT,
        domovoi_core::unit::UnitRefusal::TargetStale,
    );
    r.w.approve(op, step).await.unwrap();
    r.w.until_outcome(op, step, StepOutcome::Refused).await;
    assert!(reason(&r.w, op, step).await.starts_with("target_stale:"));
    assert!(r.relay.dispatches().is_empty());
}

#[tokio::test]
async fn dispatch_hand_off_failure_is_not_run() {
    let r = rig(Mode::Evaluated, false).await;
    let (op, step) = propose(&r).await;
    r.relay
        .fail_dispatch(LinkError::Transport("down".to_owned()));
    r.w.approve(op, step).await.unwrap();
    r.w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&r.w, op, step).await, "hub_not_handed_off");
}

#[tokio::test]
async fn result_timeout_stays_dispatched_and_reconciles() {
    let r = rig(Mode::Evaluated, false).await;
    let (op, step) = propose(&r).await;
    r.relay.queue_result(Err(LinkError::Timeout));
    r.w.approve(op, step).await.unwrap();
    r.w.until_outcome(op, step, StepOutcome::Running).await;
    // Nothing is retried after the hand-off.
    r.w.settle().await;
    assert_eq!(r.relay.dispatches().len(), 1);
    assert_eq!(r.w.outcome(op, step).await, StepOutcome::Running);
}

#[tokio::test]
async fn execute_unit_restart_commits_dispatch_before_sending() {
    let r = rig(Mode::Evaluated, false).await;
    r.relay.queue_result(verified());
    let (op, step) = propose(&r).await;
    r.w.approve(op, step).await.unwrap();
    r.w.until_outcome(op, step, StepOutcome::Succeeded).await;
    // The relay was asked to read at plan time, then to hand off, then for the result; and by the
    // time it handed off the writer had recorded the dispatch.
    let events = r.relay.events();
    let dispatch = events.iter().position(|e| e == "dispatch").unwrap();
    assert!(
        events[..dispatch].iter().all(|e| e.starts_with("read:")),
        "{events:?}"
    );
    let row = r.w.load(op).await;
    assert!(row.execution_of(step).unwrap().value.dispatched.is_some());
}

#[tokio::test]
async fn pause_blocks_dispatch_of_an_issued_permit() {
    let r = rig(Mode::Allow, true).await;
    let (op, step) = propose(&r).await;
    r.w.uow
        .commit(
            Transaction::new().write(StateWrite::Permit(PermitWrite::Pause {
                by: "owner".to_owned(),
                reason: "maintenance".to_owned(),
                at: r.w.now(),
            })),
        )
        .await
        .unwrap();
    r.w.queue.open();
    r.w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&r.w, op, step).await, "autonomy_paused");
    assert!(r.relay.dispatches().is_empty(), "no frame was sent");
}

#[tokio::test]
async fn outcome_unknown_blocks_unattended_dispatch_but_not_a_human_approval() {
    // An earlier restart of the same unit ended with an outcome nobody resolved.
    let r = rig(Mode::Evaluated, false).await;
    r.relay.queue_result(Err(LinkError::Timeout));
    let (op1, step1) = propose(&r).await;
    r.w.approve(op1, step1).await.unwrap();
    r.w.until_outcome(op1, step1, StepOutcome::Running).await;
    let progress = domovoi_core::StepProgress {
        outcome: StepOutcome::OutcomeUnknown,
        started_at: Some(r.w.now()),
        ended_at: Some(r.w.now()),
        summary: None,
        reason: Some("lost".to_owned()),
        output: None,
    };
    r.w.uow
        .commit(Transaction::new().write(StateWrite::Operation(
            domovoi_core::ports::OperationWrite::Progress {
                step: step1,
                from: StepOutcome::Running,
                to: progress,
            },
        )))
        .await
        .unwrap();

    // A permit for the same unit is refused at dispatch: the guard is keyed by the unit, so
    // a configuration change does not clear it.
    *r.issuer.mode.lock().unwrap() = Mode::Allow;
    r.w.resolver
        .set_unit(host_id(), UNIT, UNIT, 'e', "untested");
    let (op2, step2) = propose(&r).await;
    r.w.until_outcome(op2, step2, StepOutcome::Refused).await;
    assert_eq!(reason(&r.w, op2, step2).await, "unresolved_unknown");

    // A person approving the same unit is never blocked.
    *r.issuer.mode.lock().unwrap() = Mode::Evaluated;
    r.relay.queue_result(verified());
    let (op3, step3) = propose(&r).await;
    r.w.approve(op3, step3).await.unwrap();
    r.w.until_outcome(op3, step3, StepOutcome::Succeeded).await;
}
