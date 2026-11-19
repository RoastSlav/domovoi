//! The operation writer: what each command commits, and what a refusal leaves alone.

use std::sync::Arc;

use domovoi_core::ports::{AggregateKind, EffectQueue, EffectStatus, JournalKind};
use domovoi_core::{
    Actor, ApprovalProvenance, Authorization, BudgetKind, BudgetLimits, ContractId, ExecutionState,
    OperationStatus, PauseReason, PlanBinding, PlanHash, PolicyPermit, PolicyPermitParts, StepKind,
    StepOutcome, UserId,
};
use domovoi_engine::operations::{
    DecideError, ExecutionRequest, OperationError, OperationSettings,
};
use domovoi_testkit::approvals;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use crate::harness::{Flaky, restart, seed, world, world_gated, world_over, world_with};

fn kinds(entries: &[domovoi_core::ports::JournalEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.kind.as_str()).collect()
}

#[tokio::test]
async fn commit_writes_state_journal_audit_and_effects_together() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;

    let snapshot = w.load(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::WaitingApproval);
    assert_eq!(
        snapshot.steps.len(),
        2,
        "an approval step and an execution step"
    );
    assert_eq!(snapshot.steps[0].step.kind, StepKind::Approval);
    assert_eq!(snapshot.steps[1].step.id, step);
    assert_eq!(snapshot.edges.len(), 1);
    assert!(snapshot.execution_of(step).is_some());

    let journal = w.journal(op).await;
    let kinds = kinds(&journal);
    assert!(kinds.contains(&JournalKind::StepCreated.as_str()));
    assert!(kinds.contains(&JournalKind::ExecutionTransition.as_str()));
    assert!(kinds.contains(&JournalKind::OperationStatus.as_str()));
    assert!(
        journal[0].actor == "system:tests",
        "the creation is the seed's actor's"
    );
    assert!(
        journal[1..]
            .iter()
            .all(|entry| entry.actor == "system:operations")
    );

    let audit = w.audit_kinds().await;
    assert_eq!(audit, ["operation.created", "execution.proposed"]);

    let pending = w.effects(op, EffectStatus::Pending).await;
    let mut purposes: Vec<&str> = pending
        .iter()
        .map(|effect| effect.payload["purpose"].as_str().unwrap())
        .collect();
    purposes.sort_unstable();
    assert_eq!(purposes, ["approval_expiry", "wall_time"]);
    assert!(pending.iter().all(|effect| effect.kind == "wake"));
}

#[tokio::test]
async fn stale_operation_version_is_reloaded_and_retried_once() {
    let store = Arc::new(domovoi_testkit::InMemoryStore::new());
    let flaky = Flaky::new(store.clone(), 0);
    let w = world_over(OperationSettings::default(), store, flaky.clone()).await;
    let op = w.create().await;

    flaky.conflict_next(1);
    let step = w.propose(op).await;
    assert!(
        w.load(op).await.execution_of(step).is_some(),
        "the retry committed"
    );

    let before = w.load(op).await;
    flaky.conflict_next(u32::MAX);
    let refused = w.handle().propose_execution(op, restart("plex")).await;
    assert_eq!(refused, Err(OperationError::Conflict));
    assert_eq!(w.load(op).await, before, "a second conflict stores nothing");
}

#[tokio::test]
async fn authorize_requires_a_human_authorization() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    let plan = w.load(op).await.execution_of(step).unwrap().value.plan_hash;
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "dec-1",
        rule_ids: vec!["rule".to_owned()],
        policy_version: "1",
        plan_hash: plan,
        issued_at: T0,
        expires_at: T0 + SignedDuration::from_mins(5),
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .unwrap();

    let refused = w
        .handle()
        .authorize(
            op,
            step,
            Authorization::policy(permit),
            PlanBinding::Full(plan),
            ApprovalProvenance::new(None, "policy"),
        )
        .await;
    assert_eq!(
        refused,
        Err(OperationError::Execution(
            domovoi_core::ExecutionError::NotYetSupported
        ))
    );
    let row = w.load(op).await;
    assert!(row.execution_of(step).unwrap().value.approval().is_none());
    assert!(
        w.effects(op, EffectStatus::Pending)
            .await
            .iter()
            .all(|e| e.kind != "dispatch")
    );
}

#[tokio::test]
async fn approve_with_the_wrong_plan_hash_changes_nothing() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    let before = w.load(op).await;
    let audit = w.audit_kinds().await;
    let wrong: PlanHash = "ab".repeat(32).parse().unwrap();

    let refused = w
        .handle()
        .authorize(
            op,
            step,
            approvals::authorize(approvals::human(w.user)),
            PlanBinding::Full(wrong),
            approvals::provenance(),
        )
        .await;

    assert_eq!(
        refused,
        Err(OperationError::Execution(
            domovoi_core::ExecutionError::PlanChanged
        ))
    );
    assert_eq!(w.load(op).await, before);
    assert_eq!(w.audit_kinds().await, audit);
    assert!(
        w.effects(op, EffectStatus::Pending)
            .await
            .iter()
            .all(|e| e.kind != "dispatch")
    );
}

#[tokio::test]
async fn approving_creates_a_dispatch_effect_and_an_audit_row_in_one_commit() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;

    w.approve(op, step).await.unwrap();

    let snapshot = w.load(op).await;
    let approval = snapshot
        .steps
        .iter()
        .find(|record| record.step.kind == StepKind::Approval)
        .unwrap();
    assert_eq!(approval.progress.outcome, StepOutcome::Succeeded);
    let row = &snapshot.execution_of(step).unwrap().value;
    assert!(
        row.approval().is_some(),
        "the approval is on the stored row"
    );
    assert_eq!(
        row.state(StepOutcome::Pending, Some(StepOutcome::Succeeded)),
        ExecutionState::Authorized
    );
    let dispatch: Vec<_> = w
        .effects(op, EffectStatus::Pending)
        .await
        .into_iter()
        .filter(|effect| effect.kind == "dispatch")
        .collect();
    assert_eq!(dispatch.len(), 1);
    assert_eq!(
        dispatch[0].dedup_key.as_deref(),
        Some(format!("dispatch:{step}").as_str())
    );
    assert_eq!(dispatch[0].max_attempts, 5);
    assert_eq!(
        w.audit_kinds().await,
        [
            "operation.created",
            "execution.proposed",
            "execution.approved"
        ]
    );
    let approved = w
        .audit_event("execution.approved")
        .await
        .expect("the approval is audited");
    assert_eq!(approved.actor, format!("user:{}", w.user));
    let detail = approved.detail.to_string();
    assert!(detail.contains(&row.plan_hash.short()));
    assert!(
        !detail.contains(&row.plan_hash.to_string()),
        "only the short hash is audited"
    );

    let second = w.approve(op, step).await;
    assert!(second.is_err(), "a second approval is refused");
}

#[tokio::test]
async fn deny_records_refusal_and_audit() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;

    w.handle()
        .deny(op, step, approvals::human(w.user))
        .await
        .unwrap();

    let snapshot = w.load(op).await;
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Refused
    );
    assert_eq!(
        snapshot.step(step).unwrap().progress.reason.as_deref(),
        Some("denied by a person")
    );
    assert_eq!(snapshot.operation.status, OperationStatus::Running);
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.denied".to_owned())
    );
    assert!(
        w.effects(op, EffectStatus::Pending)
            .await
            .iter()
            .all(|e| e.kind != "dispatch")
    );
}

#[tokio::test]
async fn non_skill_contract_is_stored_refused_at_proposal() {
    let w = world().await;
    let op = w.create().await;
    let mut request = restart("plex");
    request.intent.contract = ContractId::parse("container.restart").unwrap();

    let step = w.handle().propose_execution(op, request).await.unwrap();

    let snapshot = w.load(op).await;
    let record = snapshot.step(step).unwrap();
    assert_eq!(record.progress.outcome, StepOutcome::Refused);
    assert!(snapshot.execution_of(step).is_none(), "no execution row");
    assert!(
        snapshot
            .steps
            .iter()
            .all(|r| r.step.kind != StepKind::Approval),
        "no approval step"
    );
    assert_eq!(snapshot.steps.len(), 1);
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.refused".to_owned())
    );
}

#[tokio::test]
async fn an_unknown_skill_or_bad_arguments_are_stored_refused() {
    let w = world().await;
    let op = w.create().await;
    let mut unknown = restart("plex");
    unknown.intent.contract = ContractId::parse("skill.nothing_like_it").unwrap();
    let mut bad = restart("Not A Container!");
    bad.intent
        .args
        .insert("container".to_owned(), "Not A Container!".to_owned());

    for request in [unknown, bad] {
        let step = w.handle().propose_execution(op, request).await.unwrap();
        assert_eq!(w.outcome(op, step).await, StepOutcome::Refused);
    }
}

#[tokio::test]
async fn a_host_without_a_live_session_stores_nothing() {
    let w = world().await;
    let op = w.create().await;
    w.link.set_presence(
        crate::harness::host_id(),
        domovoi_core::ports::Presence::Disconnected { last_seen: None },
    );
    let before = w.load(op).await;

    let refused = w.handle().propose_execution(op, restart("plex")).await;

    assert_eq!(
        refused,
        Err(OperationError::HostUnavailable(crate::harness::host_id()))
    );
    assert_eq!(w.load(op).await, before);
}

#[tokio::test]
async fn cancel_cancels_pending_effects_and_steps() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    assert_eq!(w.effects(op, EffectStatus::Pending).await.len(), 2);

    w.handle()
        .cancel(op, Actor::User(w.user), "not needed".to_owned())
        .await
        .unwrap();

    let snapshot = w.load(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Cancelled);
    assert!(snapshot.operation.ended_at.is_some());
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Cancelled
    );
    assert!(snapshot.open_steps().next().is_none());
    assert!(w.effects(op, EffectStatus::Pending).await.is_empty());
    assert_eq!(w.effects(op, EffectStatus::Cancelled).await.len(), 2);
    let cancelled = w.audit_event("operation.cancelled").await.unwrap();
    assert_eq!(cancelled.actor, format!("user:{}", w.user));
    let again = w
        .handle()
        .cancel(op, Actor::User(w.user), "again".to_owned())
        .await;
    assert_eq!(
        again,
        Err(OperationError::Decide(DecideError::OperationClosed))
    );
}

#[tokio::test]
async fn max_running_queues_the_rest_and_admits_on_completion() {
    let settings = OperationSettings {
        max_running: 1,
        ..OperationSettings::default()
    };
    let w = world_with(settings).await;
    let first = w.create().await;
    w.advance(1);
    let second = w.create().await;
    w.advance(1);
    let third = w.create().await;

    let status = |id| {
        let w = &w;
        async move { w.load(id).await.operation.status }
    };
    assert_eq!(status(first).await, OperationStatus::Running);
    assert_eq!(status(second).await, OperationStatus::Queued);
    assert_eq!(status(third).await, OperationStatus::Queued);

    w.handle()
        .cancel(first, Actor::User(w.user), "done".to_owned())
        .await
        .unwrap();
    assert_eq!(
        status(second).await,
        OperationStatus::Running,
        "oldest first"
    );
    assert_eq!(status(third).await, OperationStatus::Queued);

    let paused = w.handle().pause(second, Actor::User(w.user)).await;
    assert!(paused.is_ok());
    w.advance(1);
    let fourth = w.create().await;
    assert_eq!(
        status(fourth).await,
        OperationStatus::Queued,
        "a paused operation keeps its slot"
    );
}

#[tokio::test]
async fn budget_exhaustion_pauses() {
    let w = world().await;
    let mut limited = seed();
    limited.budgets = Some(BudgetLimits {
        executions: Some(1),
        ..BudgetLimits::default()
    });
    let op = w.handle().create(limited).await.unwrap();
    let first = w.propose(op).await;
    assert_eq!(w.outcome(op, first).await, StepOutcome::Pending);

    let second = w
        .handle()
        .propose_execution(op, restart("plex"))
        .await
        .unwrap();

    let snapshot = w.load(op).await;
    assert_eq!(
        snapshot.step(second).unwrap().progress.outcome,
        StepOutcome::Refused
    );
    assert_eq!(
        snapshot.operation.status,
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::Executions
            }
        }
    );
    let refused = w.handle().propose_execution(op, restart("plex")).await;
    assert_eq!(refused, Err(OperationError::Decide(DecideError::Paused)));
}

#[tokio::test]
async fn startup_returns_a_claimed_dispatch_effect_to_pending() {
    let mut w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    let claimed = EffectQueue::claim_due(
        &*w.store,
        "dead-runner",
        &[domovoi_core::ports::EffectKind::Dispatch],
        &[AggregateKind::Operation],
        w.now(),
        SignedDuration::from_mins(5),
        10,
    )
    .await
    .unwrap();
    assert_eq!(claimed.claimed.len(), 1);
    assert_eq!(w.effects(op, EffectStatus::Claimed).await.len(), 1);

    w.restart().await;
    w.settle().await;

    assert!(w.handle().is_alive());
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
    let pending = w.effects(op, EffectStatus::Pending).await;
    assert!(
        pending.iter().any(|effect| effect.kind == "dispatch"),
        "the claimed dispatch effect is pending again"
    );
}

#[tokio::test]
async fn a_pending_question_and_its_answer_are_one_commit_each() {
    let w = world().await;
    let op = w.create().await;
    let question = domovoi_engine::operations::NewStep {
        id: domovoi_core::StepId::new(),
        kind: StepKind::Question,
        phase: "ask".to_owned(),
        parent: None,
        intent: domovoi_core::StepIntent::Question {
            text: "Restart plex?".to_owned(),
            expires_at: T0 + SignedDuration::from_hours(1),
        },
    };
    let id = question.id;
    w.handle()
        .propose(op, vec![question], Vec::new())
        .await
        .unwrap();
    assert_eq!(
        w.load(op).await.operation.status,
        OperationStatus::WaitingUser
    );

    w.handle()
        .answer(op, id, "yes, please".to_owned(), Actor::User(UserId::new()))
        .await
        .unwrap();

    let snapshot = w.load(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Running);
    assert_eq!(
        snapshot.step(id).unwrap().progress.outcome,
        StepOutcome::Succeeded
    );
    assert_eq!(snapshot.steps.last().unwrap().step.kind, StepKind::Note);
}

#[tokio::test]
async fn a_request_for_an_unpinned_target_is_stored_refused() {
    let w = world().await;
    let op = w.create().await;
    let mut request: ExecutionRequest = restart("plex");
    request.intent.target = Some(domovoi_core::TargetKey::parse("other").unwrap());

    let step = w.handle().propose_execution(op, request).await.unwrap();

    let snapshot = w.load(op).await;
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Refused
    );
    assert_eq!(
        snapshot.step(step).unwrap().progress.reason.as_deref(),
        Some("target is not pinned to this operation")
    );
}
