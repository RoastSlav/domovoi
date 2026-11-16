//! Verification as a durable effect: created with the success transition, resumed after a
//! restart, retried with backoff and dead-lettered when it cannot run.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, AggregateRef, ClaimToken, CommitError, CommitReceipt, EffectDraft,
    EffectKind, EffectRead, EffectRecord, IncidentRepo, LinkError, StateWrite, Transaction,
    UnitOfWork,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, AdvisoryKind, EffectId, Incident, PlanHash, Principal,
    UserId, Verdict,
};
use domovoi_engine::ActionSettings;
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::actions_harness::{Actions, actions, actions_with_uow, seeded, verified_restart};
use crate::harness::host_id;
use crate::uow_probe::RecordingUow;

/// The verify delay `verified_restart` declares.
const DELAY: SignedDuration = SignedDuration::from_secs(10);

fn params() -> BTreeMap<String, String> {
    [("container".to_owned(), "nginx".to_owned())]
        .into_iter()
        .collect()
}

/// A `Succeeded` action, as the session that ran it left it in the store.
async fn succeeded_action(store: &InMemoryStore, incident: &Incident) -> ActionRequest {
    let mut action = proposed(incident);
    action.approve(human(UserId::new()), T0).expect("approved");
    let skill = verified_restart();
    let validated = skill.validate_args(&params()).expect("valid arguments");
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &validated);
    action.dispatch(plan, None, T0).expect("dispatched");
    action.complete(Ok(()), T0).expect("succeeded");
    ActionRepo::insert(store, &action)
        .await
        .expect("the action is stored");
    action
}

/// An action that already reached `Verified` in the previous session.
async fn verified_action(store: &InMemoryStore, incident: &Incident) -> ActionRequest {
    let mut action = succeeded_action(store, incident).await;
    action.verified(true, T0).expect("verified");
    ActionRepo::update_from(store, &action, "succeeded")
        .await
        .expect("the verification is stored");
    action
}

fn proposed(incident: &Incident) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&params()).expect("valid arguments");
    ActionRequest::propose(
        incident.id(),
        host_id(),
        &skill,
        &validated,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    )
}

fn verify_payload(action: ActionId) -> Value {
    json!({"v": 1, "purpose": "verify_action", "action": action.to_string()})
}

/// Stores a due `wake` effect for `action`, as the writer would have with the success.
async fn seed_wake(store: &InMemoryStore, action: ActionId, payload: Value) -> EffectId {
    let draft = EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Wake,
        aggregate: AggregateRef::action(action),
        payload,
        not_before: T0,
        max_attempts: 5,
        dedup_key: Some(format!("verify:{action}")),
        created_at: T0,
    };
    let id = draft.id;
    UnitOfWork::commit(store, Transaction::new().effect(draft))
        .await
        .expect("the wake is stored");
    id
}

async fn effect(store: &InMemoryStore, id: EffectId) -> EffectRecord {
    EffectRead::effect(store, id)
        .await
        .expect("the effect is readable")
        .expect("the effect exists")
}

/// Lets the service and its effect runner progress, waiting for a condition on the effect.
async fn effect_until(
    a: &Actions,
    id: EffectId,
    done: impl Fn(&EffectRecord) -> bool,
) -> EffectRecord {
    for _ in 0..2000 {
        a.settle().await;
        let row = effect(&a.store, id).await;
        if done(&row) {
            return row;
        }
        a.clock.advance(SignedDuration::from_secs(1));
    }
    effect(&a.store, id).await
}

fn checks(a: &Actions) -> usize {
    a.link
        .requests()
        .iter()
        .filter(|r| matches!(r, domovoi_testkit::link::LinkRequest::Check { .. }))
        .count()
}

#[tokio::test]
async fn verification_resumes_after_restart() {
    let world = seeded().await;
    let action = succeeded_action(&world.store, &world.incident).await;
    let effect_id = seed_wake(&world.store, action.id(), verify_payload(action.id())).await;

    let a = world.start(ActionSettings::default(), world.store.clone());
    a.until(action.id(), |s| s == &ActionStatus::Verified).await;

    assert_eq!(effect(&a.store, effect_id).await.status, "done");
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.verdict(), Some(Verdict::Fixed));
    assert_eq!(checks(&a), 1, "the re-check ran once");
}

#[tokio::test]
async fn verification_resumes_after_a_restart_that_held_the_claim() {
    let world = seeded().await;
    let action = succeeded_action(&world.store, &world.incident).await;
    let effect_id = seed_wake(&world.store, action.id(), verify_payload(action.id())).await;
    // The previous session claimed it and stopped: the row is left `claimed`.
    let lease = T0.checked_add(SignedDuration::from_mins(5)).unwrap_or(T0);
    world
        .store
        .claim_effect(effect_id, ClaimToken::new(), lease)
        .expect("the previous session held the claim");

    let a = world.start(ActionSettings::default(), world.store.clone());
    a.until(action.id(), |s| s == &ActionStatus::Verified).await;

    let row = effect(&a.store, effect_id).await;
    assert_eq!(row.status, "done");
    assert_eq!(
        row.claim_generation, 3,
        "the start returned the stale claim (2) and the next pass claimed it (3)"
    );
}

#[tokio::test]
async fn verification_effect_is_created_with_the_success_transition() {
    let store = Arc::new(InMemoryStore::new());
    let recorder = Arc::new(RecordingUow::new(store.clone()));
    let uow: Arc<dyn UnitOfWork> = recorder.clone();
    let a = actions_with_uow(store.clone(), uow, ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let plan = a.get(action.id()).await.plan_hash();
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            domovoi_core::PlanBinding::Full(plan),
        )
        .await
        .unwrap();
    a.until(action.id(), |s| s == &ActionStatus::Succeeded)
        .await;

    let committed = recorder
        .commits()
        .into_iter()
        .find(|tx| {
            tx.writes.iter().any(|write| {
                matches!(write, StateWrite::Action { action, .. }
                    if action.status() == &ActionStatus::Succeeded)
            })
        })
        .expect("the success transition was committed");
    assert_eq!(
        committed.new_effects.len(),
        1,
        "one wake is created with it"
    );
    let draft = &committed.new_effects[0];
    assert_eq!(draft.kind, EffectKind::Wake);
    assert_eq!(draft.aggregate, AggregateRef::action(action.id()));
    assert_eq!(draft.payload, verify_payload(action.id()));
    assert_eq!(draft.dedup_key, Some(format!("verify:{}", action.id())));
    assert_eq!(draft.max_attempts, 5);
    assert_eq!(
        draft.not_before.duration_since(draft.created_at),
        DELAY,
        "the effect is due after the skill's verify delay"
    );
}

/// Refuses the commit that would record the success, and forwards everything else.
struct RefusingSuccess {
    store: Arc<InMemoryStore>,
}

#[async_trait]
impl UnitOfWork for RefusingSuccess {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        let succeeds = tx.writes.iter().any(|write| {
            matches!(write, StateWrite::Action { action, .. }
                if action.status() == &ActionStatus::Succeeded)
        });
        if succeeds {
            return Err(CommitError::Backend("disk full".to_owned()));
        }
        UnitOfWork::commit(&*self.store, tx).await
    }
}

#[tokio::test]
async fn a_failed_success_commit_creates_no_wake() {
    let store = Arc::new(InMemoryStore::new());
    let uow: Arc<dyn UnitOfWork> = Arc::new(RefusingSuccess {
        store: store.clone(),
    });
    let a = actions_with_uow(store.clone(), uow, ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let plan = a.get(action.id()).await.plan_hash();
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            domovoi_core::PlanBinding::Full(plan),
        )
        .await
        .unwrap();
    a.until(action.id(), |s| {
        matches!(s, ActionStatus::Dispatched { .. })
    })
    .await;
    a.settle().await;

    let stored = a.get(action.id()).await;
    assert!(
        matches!(stored.status(), ActionStatus::Dispatched { .. }),
        "the refused commit changed nothing"
    );
    for status in [
        domovoi_core::ports::EffectStatus::Pending,
        domovoi_core::ports::EffectStatus::Claimed,
        domovoi_core::ports::EffectStatus::Done,
    ] {
        assert!(
            EffectRead::effects_with_status(&*a.store, status, 50)
                .await
                .unwrap()
                .is_empty(),
            "no effect survives a refused commit"
        );
    }
}

#[tokio::test]
async fn verification_waits_for_its_delay_without_a_task_sleeping() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let plan = a.get(action.id()).await.plan_hash();
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            domovoi_core::PlanBinding::Full(plan),
        )
        .await
        .unwrap();
    let done = a
        .until(action.id(), |s| s == &ActionStatus::Succeeded)
        .await;
    assert_eq!(done.status(), &ActionStatus::Succeeded);

    a.settle().await;
    assert_eq!(checks(&a), 0, "nothing re-checks before the delay passes");
    let due = a.now().checked_add(DELAY).unwrap_or(a.now());
    a.clock.set(due);
    a.settle().await;
    a.until(action.id(), |s| s == &ActionStatus::Verified).await;
    assert_eq!(checks(&a), 1);
}

#[tokio::test]
async fn verification_retries_when_agent_is_offline_then_gives_a_result() {
    let a = actions(ActionSettings::default()).await;
    a.link.on_check(|_, _, _| Err(LinkError::NotConnected));
    let (action, _) = a.propose().await;
    let plan = a.get(action.id()).await.plan_hash();
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            domovoi_core::PlanBinding::Full(plan),
        )
        .await
        .unwrap();
    let done = a
        .until(action.id(), |s| s == &ActionStatus::VerifyFailed)
        .await;
    assert_eq!(done.status(), &ActionStatus::VerifyFailed);

    let filter = domovoi_core::ports::AuditFilter {
        subject: Some(format!("action:{}", action.id())),
        kind: Some("action.verify_failed".to_owned()),
        ..domovoi_core::ports::AuditFilter::default()
    };
    let entry = domovoi_core::ports::AuditLog::list(
        &*a.store,
        &filter,
        &domovoi_core::ports::Page::first(1),
    )
    .await
    .unwrap()
    .items
    .remove(0);
    assert!(
        entry.detail["check"]
            .as_str()
            .is_some_and(|why| why.contains("the re-check could not run")),
        "{}",
        entry.detail
    );

    let effects =
        EffectRead::effects_with_status(&*a.store, domovoi_core::ports::EffectStatus::Done, 50)
            .await
            .unwrap();
    assert_eq!(
        effects.len(),
        1,
        "the wake ends as a result, not a dead letter"
    );
}

#[tokio::test]
async fn verification_skipped_when_action_is_not_succeeded() {
    let world = seeded().await;
    let action = verified_action(&world.store, &world.incident).await;
    let effect_id = seed_wake(&world.store, action.id(), verify_payload(action.id())).await;

    let a = world.start(ActionSettings::default(), world.store.clone());
    let row = effect_until(&a, effect_id, |row| row.status == "done").await;

    assert_eq!(
        row.status, "done",
        "the wake finishes without a state change"
    );
    assert_eq!(
        checks(&a),
        0,
        "an already verified action is not re-checked"
    );
    let stored = a.get(action.id()).await;
    assert_eq!(stored.status(), &ActionStatus::Verified);
}

#[tokio::test]
async fn unreadable_wake_payload_is_dead_lettered() {
    let world = seeded().await;
    let action = succeeded_action(&world.store, &world.incident).await;
    let effect_id = seed_wake(
        &world.store,
        action.id(),
        json!({"v": 1, "purpose": "something_else"}),
    )
    .await;

    let a = world.start(ActionSettings::default(), world.store.clone());
    let row = effect_until(&a, effect_id, |row| row.status == "dead").await;

    assert_eq!(row.status, "dead");
    assert_eq!(
        row.last_error.as_deref(),
        Some("unreadable wake payload"),
        "the reason names the payload, not its contents"
    );
    let open = AdvisoryRepo::list_open(&*a.store).await.unwrap();
    assert!(
        open.iter()
            .any(|advisory| advisory.kind == AdvisoryKind::EffectDead),
        "a dead wake raises the advisory"
    );
    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::Succeeded,
        "nothing changed the action"
    );
}
