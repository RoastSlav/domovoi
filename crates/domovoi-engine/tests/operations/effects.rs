//! The operation runner's handlers: evidence collection, reconciliation requests and the scope
//! of what the runner claims.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use domovoi_core::ports::{
    AggregateRef, EffectDraft, EffectKind, EffectRead, EffectStatus, HostReconciler, LinkError,
    Presence, ToolOutput, Transaction,
};
use domovoi_core::{ActionId, EffectId, StepId, StepIntent, StepKind, StepOutcome};
use domovoi_engine::operations::NewStep;
use domovoi_testkit::build::T0;
use serde_json::json;

use crate::harness::{World, host_id, world};

/// An evidence request for the host overview.
fn evidence_step() -> NewStep {
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

/// Proposes one evidence request and returns its step.
async fn ask_for_evidence(w: &World, op: domovoi_core::OperationId) -> StepId {
    let ids = w
        .handle()
        .propose(op, vec![evidence_step()], Vec::new())
        .await
        .unwrap();
    ids[0]
}

fn overview(text: &str) -> Result<ToolOutput, LinkError> {
    Ok(ToolOutput {
        text: text.to_owned(),
        truncated: false,
    })
}

#[tokio::test]
async fn evidence_unavailable_host_settles_unavailable_and_transport_error_retries() {
    let w = world().await;
    let op = w.create().await;

    // A host with no agent connected: the read is unavailable, with the link's own words.
    w.link.set_presence(
        host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    let gone = ask_for_evidence(&w, op).await;
    let snapshot = w.until_outcome(op, gone, StepOutcome::Unavailable).await;
    assert_eq!(
        snapshot.step(gone).unwrap().progress.reason.as_deref(),
        Some("agent not connected")
    );

    // A transport error is tried again; the second attempt works.
    w.link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    let calls = Arc::new(AtomicU32::new(0));
    let seen = calls.clone();
    w.link.on_tool(move |_, _| {
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(LinkError::Transport("connection reset".to_owned()))
        } else {
            overview("hostname nas, up 12 days")
        }
    });
    let flaky = ask_for_evidence(&w, op).await;
    let snapshot = w.until_outcome(op, flaky, StepOutcome::Succeeded).await;
    let progress = &snapshot.step(flaky).unwrap().progress;
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one failure, one success");
    assert_eq!(
        progress.summary.as_deref(),
        Some("hostname nas, up 12 days")
    );
    assert_eq!(progress.output.as_deref(), Some("hostname nas, up 12 days"));

    // A transport error that never clears ends the step as failed after the last attempt.
    w.link
        .on_tool(|_, _| Err(LinkError::Transport("connection reset".to_owned())));
    let broken = ask_for_evidence(&w, op).await;
    let snapshot = w.until_outcome(op, broken, StepOutcome::Failed).await;
    assert!(
        snapshot
            .step(broken)
            .unwrap()
            .progress
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("connection reset"))
    );
}

#[tokio::test]
async fn evidence_output_is_redacted_before_it_is_stored() {
    let w = world().await;
    w.link.on_tool(|_, _| overview("token is hunter2 today"));
    let op = w.create().await;
    let step = ask_for_evidence(&w, op).await;

    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let progress = &snapshot.step(step).unwrap().progress;
    assert!(
        !progress
            .output
            .clone()
            .unwrap_or_default()
            .contains("hunter2")
    );
    assert!(
        !progress
            .summary
            .clone()
            .unwrap_or_default()
            .contains("hunter2")
    );
}

#[tokio::test]
async fn hello_triggered_reconcile_creates_one_effect_per_dispatched_execution() {
    let w = world().await;
    // The runs go out and no answer comes back, so both stay dispatched.
    w.link
        .on_skill(|_, _| Err(LinkError::Indeterminate("link dropped".to_owned())));
    let op = w.create().await;
    let mut steps = Vec::new();
    for _ in 0..2 {
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::Running).await;
        steps.push(step);
    }
    w.queue.close();
    w.settle().await;

    w.handle().reconcile_host(host_id()).await;
    w.handle().reconcile_host(host_id()).await;

    let mut waiting = Vec::new();
    for status in [EffectStatus::Pending, EffectStatus::Claimed] {
        waiting.extend(w.effects(op, status).await);
    }
    let mut reconciles: Vec<String> = waiting
        .iter()
        .filter(|effect| effect.kind == "reconcile")
        .map(|effect| effect.payload["step"].as_str().unwrap().to_owned())
        .collect();
    reconciles.sort();
    let mut expected: Vec<String> = steps.iter().map(ToString::to_string).collect();
    expected.sort();
    assert_eq!(reconciles, expected, "one effect for each dispatched run");
}

#[tokio::test]
async fn operation_runner_never_claims_legacy_wake_effects() {
    let w = world().await;
    // A wake of the action service: same kind, due now, but another aggregate.
    let action = ActionId::new();
    let id = EffectId::new();
    let draft = EffectDraft {
        id,
        kind: EffectKind::Wake,
        aggregate: AggregateRef::action(action),
        payload: json!({ "v": 1, "purpose": "verify_action", "action": action.to_string() }),
        not_before: T0,
        max_attempts: 3,
        dedup_key: None,
        created_at: T0,
    };
    w.uow
        .commit(Transaction::new().effect(draft))
        .await
        .unwrap();

    for _ in 0..60 {
        w.settle().await;
        w.advance(5);
    }

    let stored = EffectRead::effect(&*w.store, id).await.unwrap().unwrap();
    assert_eq!(stored.status, EffectStatus::Pending.as_str());
    assert_eq!(stored.attempt, 0, "it was never claimed");
    assert_eq!(stored.claim_generation, 0);
}
