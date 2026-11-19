use std::sync::Arc;

use axum::http::StatusCode;
use domovoi_core::IncidentId;
use domovoi_core::ports::{IncidentRepo, OperationRepo};
use domovoi_testkit::{GatewayCall, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};

use super::harness::{Harness, REPORT, guard, until};
use super::{firing, post_count};

fn start() -> Timestamp {
    "2026-11-19T12:00:00Z".parse().unwrap()
}

#[tokio::test]
async fn flag_off_starts_no_operation_service() {
    let harness = Harness::new(
        "[orchestration]
enabled = false",
        start(),
    )
    .await;

    assert!(harness.runtime.as_ref().unwrap().operations_task.is_none());

    harness.stop().await;
}

#[tokio::test]
async fn the_operation_service_and_the_composite_reconciler_start_by_default() {
    let harness = Harness::new("", start()).await;

    // The composite itself is covered in `orchestration.rs`; here the service is running
    // and stops with the hub, which `stop` proves by joining its task.
    assert!(harness.runtime.as_ref().unwrap().operations_task.is_some());

    harness.stop().await;
}

/// A hub with `extra` config, one scripted report, and the dispatcher running.
async fn triaging_hub(extra: &str) -> (Harness, Arc<ScriptedLlm>) {
    let llm = Arc::new(ScriptedLlm::new());
    llm.push_text(REPORT);
    let mut harness = Harness::with_adapters(extra, start(), llm.clone(), Vec::new(), 4096).await;
    harness.start_dispatcher();
    (harness, llm)
}

async fn operations_of(harness: &Harness, incident: IncidentId) -> usize {
    OperationRepo::for_incident(&*harness.runtime.as_ref().unwrap().store, incident)
        .await
        .unwrap()
        .len()
}

async fn timeline_count(harness: &Harness, incident: IncidentId, kind: &str) -> usize {
    IncidentRepo::timeline(&*harness.runtime.as_ref().unwrap().store, incident)
        .await
        .unwrap()
        .iter()
        .filter(|entry| entry.kind == kind)
        .count()
}

#[tokio::test]
async fn incident_is_investigated_by_default() {
    let (harness, llm) = triaging_hub("[triage]\nenabled = true\nconcurrency = 1").await;
    assert_eq!(
        harness.webhook(&firing("web-1", "backup.nightly")).await,
        StatusCode::ACCEPTED
    );
    // The investigation's effects run against the fake clock, so it moves while we wait.
    guard(async {
        while !harness.gateway.calls().iter().any(|call| {
            matches!(call, GatewayCall::UpdateCard(_, card) if card.headline == "Backup needs attention")
        }) {
            harness.clock.advance(SignedDuration::from_secs(1));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;

    let incidents = harness.incidents().await;
    assert_eq!(incidents.len(), 1);
    let id = incidents[0].id();
    assert_eq!(operations_of(&harness, id).await, 1, "one investigation");
    assert_eq!(timeline_count(&harness, id, "triage.report").await, 1);
    assert_eq!(llm.requests().len(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn orchestration_off_runs_no_automatic_triage() {
    let (harness, llm) =
        triaging_hub("[triage]\nenabled = true\nconcurrency = 1\n[orchestration]\nenabled = false")
            .await;
    assert_eq!(
        harness.webhook(&firing("web-1", "backup.nightly")).await,
        StatusCode::ACCEPTED
    );
    // The untriaged card goes out; nothing then updates it.
    until(|| post_count(&harness.gateway) == 1).await;
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }

    let incidents = harness.incidents().await;
    assert_eq!(incidents.len(), 1);
    let id = incidents[0].id();
    assert_eq!(operations_of(&harness, id).await, 0);
    assert_eq!(timeline_count(&harness, id, "triage.report").await, 0);
    assert!(llm.requests().is_empty(), "the model was never asked");
    assert!(
        !harness
            .gateway
            .calls()
            .iter()
            .any(|call| matches!(call, GatewayCall::UpdateCard(..))),
        "the card stays untriaged"
    );
    harness.stop().await;
}
