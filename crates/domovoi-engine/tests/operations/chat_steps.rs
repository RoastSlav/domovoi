//! Single-use chat tokens for steps awaiting approval, and the risk gate behind them.

use domovoi_core::ports::{
    AggregateRef, AuditFilter, AuditLog, EffectStatus, IncidentRepo, JournalRead, Page,
};
use domovoi_core::{
    ExecutionError, ExecutionId, ExecutionRecord, OperationId, PlanBinding, Risk, StepId,
};
use domovoi_engine::chat_token::{mint_token, token_hash};
use domovoi_engine::events::EventKind;
use domovoi_engine::operations::OperationError;
use domovoi_testkit::{approvals, build};

use crate::harness::{World, host_id, restart_skill, seed, world};

async fn waiting(w: &World) -> (OperationId, StepId) {
    let op = w.create().await;
    let step = w.propose(op).await;
    (op, step)
}

async fn rotated(w: &World, op: OperationId, step: StepId) -> String {
    let token = mint_token();
    w.handle()
        .rotate_chat_token(op, step, token_hash(&token))
        .await
        .unwrap();
    token
}

async fn record(w: &World, op: OperationId, step: StepId) -> ExecutionRecord {
    w.load(op).await.execution_of(step).unwrap().value.clone()
}

fn prefix_of(record: &ExecutionRecord) -> PlanBinding {
    PlanBinding::Prefix(record.plan_hash.as_bytes()[..3].to_vec())
}

fn wrong_prefix() -> PlanBinding {
    PlanBinding::Prefix(vec![0, 0, 0])
}

async fn chat_press(
    w: &World,
    op: OperationId,
    step: StepId,
    binding: PlanBinding,
    token: Option<&str>,
) -> Result<(), OperationError> {
    let approval = approvals::chat(w.user, "discord", "42");
    w.handle()
        .authorize(
            op,
            step,
            approvals::authorize(approval),
            binding,
            approvals::provenance(),
            token,
        )
        .await
}

fn destructive() -> domovoi_core::SkillSummary {
    let mut skill = restart_skill();
    skill.risk = Risk::Destructive;
    skill
}

#[tokio::test]
async fn chat_authorize_with_the_right_token_and_prefix_approves_and_spends_the_token() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    let binding = prefix_of(&record(&w, op, step).await);
    chat_press(&w, op, step, binding, Some(&token))
        .await
        .unwrap();
    let stored = record(&w, op, step).await;
    assert!(stored.approval().is_some());
    assert!(stored.chat_token_hash().is_none());
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.approved".to_owned())
    );
}

#[tokio::test]
async fn chat_authorize_with_a_wrong_token_is_refused_and_changes_nothing() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    let binding = prefix_of(&record(&w, op, step).await);
    let err = chat_press(&w, op, step, binding.clone(), Some(&mint_token()))
        .await
        .unwrap_err();
    assert_eq!(err, OperationError::BadToken);
    assert!(record(&w, op, step).await.approval().is_none());
    chat_press(&w, op, step, binding, Some(&token))
        .await
        .unwrap();
}

#[tokio::test]
async fn chat_authorize_with_a_wrong_prefix_keeps_the_token() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    let err = chat_press(&w, op, step, wrong_prefix(), Some(&token))
        .await
        .unwrap_err();
    assert_eq!(err, OperationError::Execution(ExecutionError::PlanChanged));
    assert!(record(&w, op, step).await.chat_token_hash().is_some());
    let binding = prefix_of(&record(&w, op, step).await);
    chat_press(&w, op, step, binding, Some(&token))
        .await
        .unwrap();
}

#[tokio::test]
async fn chat_authorize_checks_the_token_before_the_prefix() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    rotated(&w, op, step).await;
    let err = chat_press(&w, op, step, wrong_prefix(), Some(&mint_token()))
        .await
        .unwrap_err();
    assert_eq!(err, OperationError::BadToken);
}

#[tokio::test]
async fn chat_authorize_without_any_token_is_refused() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    rotated(&w, op, step).await;
    let binding = prefix_of(&record(&w, op, step).await);
    let err = chat_press(&w, op, step, binding, None).await.unwrap_err();
    assert_eq!(err, OperationError::BadToken);
}

#[tokio::test]
async fn web_authorize_ignores_the_chat_token() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    rotated(&w, op, step).await;
    w.approve(op, step).await.unwrap();
    let stored = record(&w, op, step).await;
    assert!(stored.approval().is_some());
    assert!(stored.chat_token_hash().is_none());
}

#[tokio::test]
async fn proposal_stores_the_skills_risk() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    assert_eq!(record(&w, op, step).await.risk(), Some(Risk::Write));
    w.link.set_skills(host_id(), vec![destructive()]);
    let (op, step) = waiting(&w).await;
    assert_eq!(record(&w, op, step).await.risk(), Some(Risk::Destructive));
}

#[tokio::test]
async fn chat_authorize_of_a_destructive_step_is_refused_by_the_record() {
    let w = world().await;
    w.link.set_skills(host_id(), vec![destructive()]);
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    let binding = prefix_of(&record(&w, op, step).await);
    let err = chat_press(&w, op, step, binding, Some(&token))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        OperationError::Execution(ExecutionError::NeedsTypedConfirmation)
    );
    assert!(record(&w, op, step).await.approval().is_none());
}

#[tokio::test]
async fn web_authorize_of_a_destructive_step_needs_typed_confirmation() {
    let w = world().await;
    w.link.set_skills(host_id(), vec![destructive()]);
    let (op, step) = waiting(&w).await;
    let err = w.approve(op, step).await.unwrap_err();
    assert_eq!(
        err,
        OperationError::Execution(ExecutionError::NeedsTypedConfirmation)
    );
    let binding = w.binding(op, step).await;
    w.handle()
        .authorize(
            op,
            step,
            approvals::authorize(approvals::web_typed(w.user)),
            binding,
            approvals::provenance(),
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn rotate_replaces_the_token_and_the_old_one_stops_working() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let first = rotated(&w, op, step).await;
    let second = rotated(&w, op, step).await;
    let binding = prefix_of(&record(&w, op, step).await);
    let err = chat_press(&w, op, step, binding.clone(), Some(&first))
        .await
        .unwrap_err();
    assert_eq!(err, OperationError::BadToken);
    chat_press(&w, op, step, binding, Some(&second))
        .await
        .unwrap();
}

#[tokio::test]
async fn rotate_is_refused_for_a_decided_or_expired_step() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    w.approve(op, step).await.unwrap();
    let decided = w.handle().rotate_chat_token(op, step, [1; 32]).await;
    assert!(matches!(decided, Err(OperationError::Decide(_))));
    let (op, step) = waiting(&w).await;
    w.advance(48 * 3600);
    let late = w.handle().rotate_chat_token(op, step, [1; 32]).await;
    assert_eq!(
        late,
        Err(OperationError::Execution(ExecutionError::Expired))
    );
    assert!(record(&w, op, step).await.chat_token_hash().is_none());
}

#[tokio::test]
async fn token_is_cleared_when_the_step_is_denied_or_cancelled() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    w.handle()
        .deny(op, step, approvals::human(w.user))
        .await
        .unwrap();
    assert!(record(&w, op, step).await.chat_token_hash().is_none());
    let binding = prefix_of(&record(&w, op, step).await);
    assert!(
        chat_press(&w, op, step, binding, Some(&token))
            .await
            .is_err()
    );

    let (op, step) = waiting(&w).await;
    rotated(&w, op, step).await;
    w.handle()
        .cancel(op, domovoi_core::Actor::User(w.user), "no".to_owned())
        .await
        .unwrap();
    assert!(record(&w, op, step).await.chat_token_hash().is_none());
}

#[tokio::test]
async fn chat_token_never_reaches_audit_journal_or_effects() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let token = rotated(&w, op, step).await;
    let hash_hex = hex::encode(token_hash(&token));
    let binding = prefix_of(&record(&w, op, step).await);
    chat_press(&w, op, step, binding, Some(&token))
        .await
        .unwrap();
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.token_rotated".to_owned())
    );
    let page = AuditLog::list(&*w.store, &AuditFilter::default(), &Page::first(500))
        .await
        .unwrap();
    let mut text = format!("{:?}", page.items);
    let journal = JournalRead::entries_for(&*w.store, &AggregateRef::operation(op), 500)
        .await
        .unwrap();
    text.push_str(&format!("{journal:?}"));
    for status in [
        EffectStatus::Pending,
        EffectStatus::Claimed,
        EffectStatus::Done,
    ] {
        text.push_str(&format!("{:?}", w.effects(op, status).await));
    }
    assert!(!text.contains(&token));
    assert!(!text.contains(&hash_hex));
}

#[tokio::test]
async fn deciding_a_step_publishes_execution_changed() {
    let w = world().await;
    let incident = build::open_incident(host_id(), "container.down", Some("plex")).unwrap();
    IncidentRepo::insert(&*w.store, &incident).await.unwrap();
    let mut s = seed();
    s.incidents = vec![incident.id()];
    let op = w.handle().create(s).await.unwrap();
    let step = w.propose(op).await;
    let mut rx = w.bus.subscribe();
    w.approve(op, step).await.unwrap();
    let event = rx.try_recv().unwrap();
    assert!(matches!(
        event.kind,
        EventKind::ExecutionChanged { execution } if execution == ExecutionId::from_step(step)
    ));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn chat_deny_checks_and_spends_the_token() {
    let w = world().await;
    let (op, step) = waiting(&w).await;
    let old = rotated(&w, op, step).await;
    let current = rotated(&w, op, step).await;
    let human = || approvals::chat(w.user, "discord", "42");
    for bad in [mint_token(), old] {
        let err = w
            .handle()
            .deny_from_chat(op, step, human(), &bad)
            .await
            .unwrap_err();
        assert_eq!(err, OperationError::BadToken);
    }
    assert!(record(&w, op, step).await.chat_token_hash().is_some());
    w.handle()
        .deny_from_chat(op, step, human(), &current)
        .await
        .unwrap();
    assert!(record(&w, op, step).await.chat_token_hash().is_none());
    assert!(
        w.handle()
            .deny_from_chat(op, step, human(), &current)
            .await
            .is_err()
    );
}
