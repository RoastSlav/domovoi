//! Work that must not be lost: seed steps of a queued operation, effects addressed to another
//! operation, and a start-up with more claimed effects than one read returns.

use std::sync::Arc;

use domovoi_core::ports::{
    AggregateKind, AggregateRef, EffectDraft, EffectKind, EffectQueue, EffectRead, EffectStatus,
    ToolOutput, Transaction,
};
use domovoi_core::{
    ActionId, Actor, EffectId, OperationId, OperationStatus, StepId, StepIntent, StepKind,
    StepOutcome,
};
use domovoi_engine::operations::{NewStep, OperationSettings};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use serde_json::json;

use crate::harness::{World, host_id, seed, world_gated, world_with};

/// A world where one operation fills the only slot.
async fn one_slot() -> (World, OperationId) {
    let w = world_with(OperationSettings {
        max_running: 1,
        ..OperationSettings::default()
    })
    .await;
    let first = w.create().await;
    w.advance(1);
    (w, first)
}

fn evidence() -> NewStep {
    NewStep {
        id: StepId::new(),
        kind: StepKind::EvidenceRequest,
        phase: "evidence".to_owned(),
        parent: None,
        intent: StepIntent::EvidenceRequest {
            host: host_id(),
            tool: "host_overview".to_owned(),
            args: json!({}),
        },
    }
}

fn question(w: &World, wait: i64) -> NewStep {
    NewStep {
        id: StepId::new(),
        kind: StepKind::Question,
        phase: "ask".to_owned(),
        parent: None,
        intent: StepIntent::Question {
            text: "restart plex?".to_owned(),
            expires_at: w.now() + SignedDuration::from_secs(wait),
        },
    }
}

#[tokio::test]
async fn queued_operation_keeps_its_evidence_request_until_it_is_admitted() {
    let (w, first) = one_slot().await;
    w.link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "hostname nas".to_owned(),
            truncated: false,
        })
    });
    let step = evidence();
    let id = step.id;
    let mut queued = seed();
    queued.first_steps = vec![step];
    let second = w.handle().create(queued).await.unwrap();
    assert_eq!(
        w.load(second).await.operation.status,
        OperationStatus::Queued
    );

    // The runner tries for a while: the read does not run and the step is not dropped.
    for _ in 0..30 {
        w.settle().await;
        w.advance(5);
    }
    let snapshot = w.load(second).await;
    assert_eq!(
        snapshot.step(id).unwrap().progress.outcome,
        StepOutcome::Pending
    );
    let waiting = w.effects(second, EffectStatus::Pending).await.len()
        + w.effects(second, EffectStatus::Claimed).await.len();
    assert!(waiting > 0, "the evidence effect is still waiting");
    assert!(w.effects(second, EffectStatus::Dead).await.is_empty());

    // A slot frees up: the second operation is admitted and its read runs.
    w.handle()
        .cancel(first, Actor::User(w.user), "done".to_owned())
        .await
        .unwrap();
    let snapshot = w.until_outcome(second, id, StepOutcome::Succeeded).await;
    assert_eq!(
        snapshot.step(id).unwrap().progress.summary.as_deref(),
        Some("hostname nas")
    );
}

#[tokio::test]
async fn queued_operation_keeps_its_question_timeout_until_it_is_admitted() {
    let (w, first) = one_slot().await;
    let step = question(&w, 60);
    let id = step.id;
    let mut queued = seed();
    queued.first_steps = vec![step];
    let second = w.handle().create(queued).await.unwrap();

    // The timeout passes while the operation has no slot.
    for _ in 0..30 {
        w.settle().await;
        w.advance(10);
    }
    let snapshot = w.load(second).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Queued);
    assert_eq!(
        snapshot.step(id).unwrap().progress.outcome,
        StepOutcome::Pending
    );

    w.handle()
        .cancel(first, Actor::User(w.user), "done".to_owned())
        .await
        .unwrap();
    let snapshot = w.until_outcome(second, id, StepOutcome::NotRun).await;
    assert_eq!(
        snapshot.step(id).unwrap().progress.reason.as_deref(),
        Some("no answer before the timeout")
    );
}

/// A wake of operation `aggregate` whose payload names `named`.
fn forged_wake(aggregate: OperationId, named: OperationId, step: StepId, v: u64) -> EffectDraft {
    EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Wake,
        aggregate: AggregateRef::operation(aggregate),
        payload: json!({
            "v": v,
            "purpose": "question_timeout",
            "operation": named.to_string(),
            "step": step.to_string(),
        }),
        not_before: T0,
        max_attempts: 3,
        dedup_key: None,
        created_at: T0,
    }
}

#[tokio::test]
async fn effect_whose_payload_names_another_operation_is_dead_and_changes_nothing() {
    let w = world_gated().await;
    let a = w.create().await;
    let step = question(&w, 3600);
    let id = step.id;
    let mut other = seed();
    other.first_steps = vec![step];
    let b = w.handle().create(other).await.unwrap();

    let foreign = forged_wake(a, b, id, 1);
    let wrong_version = forged_wake(b, b, id, 2);
    let (foreign_id, wrong_id) = (foreign.id, wrong_version.id);
    w.uow
        .commit(Transaction::new().effect(foreign).effect(wrong_version))
        .await
        .unwrap();
    w.queue.open();
    for _ in 0..30 {
        w.settle().await;
        w.advance(1);
    }

    for effect in [foreign_id, wrong_id] {
        let stored = EffectRead::effect(&*w.store, effect)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, EffectStatus::Dead.as_str());
        assert_eq!(stored.last_error.as_deref(), Some("unreadable payload"));
    }
    let snapshot = w.load(b).await;
    assert_eq!(
        snapshot.step(id).unwrap().progress.outcome,
        StepOutcome::Pending
    );
    assert_eq!(snapshot.operation.status, OperationStatus::WaitingUser);
}

#[tokio::test]
async fn start_returns_every_claimed_operation_dispatch_effect_to_pending() {
    let mut w = world_gated().await;
    let draft = |aggregate: AggregateRef| EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Dispatch,
        aggregate,
        payload: json!({}),
        not_before: T0,
        max_attempts: 5,
        dedup_key: None,
        created_at: T0,
    };
    // More legacy claimed dispatch effects than one read returns, then the operation's own.
    let mut tx = Transaction::new();
    for _ in 0..520 {
        tx = tx.effect(draft(AggregateRef::action(ActionId::new())));
    }
    w.uow.commit(tx).await.unwrap();
    let mut ours = Vec::new();
    let mut tx = Transaction::new();
    for _ in 0..3 {
        let effect = draft(AggregateRef::operation(OperationId::new()));
        ours.push(effect.id);
        tx = tx.effect(effect);
    }
    w.uow.commit(tx).await.unwrap();
    let store = Arc::clone(&w.store);
    let lease = SignedDuration::from_secs(600);
    let legacy = store
        .claim_due(
            "old",
            &[EffectKind::Dispatch],
            &AggregateKind::LEGACY,
            T0,
            lease,
            600,
        )
        .await
        .unwrap();
    assert_eq!(legacy.claimed.len(), 520);
    let claimed = store
        .claim_due(
            "old",
            &[EffectKind::Dispatch],
            &[AggregateKind::Operation],
            T0,
            lease,
            10,
        )
        .await
        .unwrap();
    assert_eq!(claimed.claimed.len(), 3);

    w.restart().await;
    w.settle().await;

    for id in ours {
        let stored = EffectRead::effect(&*store, id).await.unwrap().unwrap();
        assert_eq!(
            stored.status,
            EffectStatus::Pending.as_str(),
            "returned at start"
        );
    }
    let still = EffectRead::effects_with_status(&*store, EffectStatus::Claimed, 500)
        .await
        .unwrap();
    assert_eq!(still.len(), 500, "the legacy dispatch effects stay claimed");
}

#[tokio::test]
async fn start_returns_more_than_one_page_of_claimed_operation_dispatch_effects() {
    let mut w = world_gated().await;
    let mut tx = Transaction::new();
    let mut ours = Vec::new();
    for _ in 0..620 {
        let effect = EffectDraft {
            id: EffectId::new(),
            kind: EffectKind::Dispatch,
            aggregate: AggregateRef::operation(OperationId::new()),
            payload: json!({}),
            not_before: T0,
            max_attempts: 5,
            dedup_key: None,
            created_at: T0,
        };
        ours.push(effect.id);
        tx = tx.effect(effect);
    }
    w.uow.commit(tx).await.unwrap();
    let store = Arc::clone(&w.store);
    let claimed = store
        .claim_due(
            "old",
            &[EffectKind::Dispatch],
            &[AggregateKind::Operation],
            T0,
            SignedDuration::from_secs(600),
            700,
        )
        .await
        .unwrap();
    assert_eq!(claimed.claimed.len(), 620);

    w.restart().await;
    w.settle().await;

    for id in ours {
        let stored = EffectRead::effect(&*store, id).await.unwrap().unwrap();
        assert_eq!(stored.status, EffectStatus::Pending.as_str());
    }
}
