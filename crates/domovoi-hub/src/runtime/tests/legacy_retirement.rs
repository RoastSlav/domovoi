//! The hub retires the actions the old executor left open before any service starts, and
//! nothing that survives needs that executor.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use domovoi_core::audit_chain::{ChainStatus, VerifyScope};
use domovoi_core::ports::{ActionFilter, ActionRepo, AuditFilter, AuditLog, Clock, Page};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, Incident, Principal, Risk};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{key_fingerprint, manifest};
use jiff::{SignedDuration, Timestamp};
use tower::ServiceExt;

use super::harness::{Harness, guard};
use super::{at, firing, wait_for_incidents};

fn start() -> Timestamp {
    at("2026-11-19T12:00:00Z")
}

/// A `restart_container` request for `incident`, still proposed.
fn proposal(incident: &Incident, now: Timestamp) -> ActionRequest {
    let summary = manifest("restart_container", Risk::Write)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"));
    let args = summary.validate_args(&BTreeMap::new()).unwrap();
    ActionRequest::propose(
        incident,
        &summary,
        &args,
        Some(key_fingerprint()),
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        now,
    )
    .unwrap()
}

/// A proposal a person approved.
fn approved(incident: &Incident, now: Timestamp) -> ActionRequest {
    let mut action = proposal(incident, now);
    domovoi_testkit::approvals::approve(&mut action, human(domovoi_core::UserId::new()), now)
        .unwrap();
    action
}

/// Stores one proposed, one approved and one dispatched legacy action on `incident`, as the
/// old executor would have left them, and answers their ids in that order.
async fn seed_open_actions(harness: &Harness, incident: &Incident) -> [ActionId; 3] {
    let store = harness.runtime.as_ref().unwrap().store.clone();
    let now = harness.clock.now();
    let proposed = proposal(incident, now);
    let waiting = approved(incident, now);
    let mut running = approved(incident, now);
    running
        .dispatch(running.plan_hash_for_approval(), None, now)
        .unwrap();
    for action in [&proposed, &waiting, &running] {
        ActionRepo::insert(&*store, action).await.unwrap();
    }
    [proposed.id(), waiting.id(), running.id()]
}

async fn status_of(harness: &Harness, id: ActionId) -> ActionStatus {
    let store = harness.runtime.as_ref().unwrap().store.clone();
    ActionRepo::get(&*store, id)
        .await
        .unwrap()
        .unwrap()
        .status()
        .clone()
}

async fn open_count(harness: &Harness) -> usize {
    let store = harness.runtime.as_ref().unwrap().store.clone();
    let mut open = 0;
    for status in ["proposed", "approved", "dispatched"] {
        let filter = ActionFilter {
            status: Some(status.to_owned()),
            ..ActionFilter::default()
        };
        open += ActionRepo::list(&*store, &filter, &Page::first(Page::MAX_LIMIT))
            .await
            .unwrap()
            .items
            .len();
    }
    open
}

async fn readyz(harness: &Harness) -> (StatusCode, String) {
    let response = harness
        .runtime
        .as_ref()
        .unwrap()
        .app
        .clone()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// A hub with one incident and three open legacy actions stored, stopped and built again so
/// the next assembly finds them, and the ids of those actions.
async fn restarted_with_open_actions(extra: &str) -> (Harness, [ActionId; 3]) {
    let harness = Harness::new(extra, start()).await;
    assert_eq!(
        harness.webhook(&firing("web-1", "backup.nightly")).await,
        StatusCode::ACCEPTED
    );
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    let ids = seed_open_actions(&harness, &incident).await;
    assert_eq!(open_count(&harness).await, 3, "the seed is open");
    (harness.restart().await, ids)
}

fn assert_retired(proposed: &ActionStatus, waiting: &ActionStatus, running: &ActionStatus) {
    assert_eq!(proposed, &ActionStatus::Expired);
    assert!(
        matches!(waiting, ActionStatus::NotRun { .. }),
        "{waiting:?}"
    );
    assert!(
        matches!(running, ActionStatus::OutcomeUnknown { .. }),
        "{running:?}"
    );
}

#[tokio::test]
async fn drain_runs_before_any_service_starts() {
    let (harness, [proposed, waiting, running]) = restarted_with_open_actions("").await;

    // The hub answers; by the time it does, no legacy action is open.
    assert_eq!(readyz(&harness).await.0, StatusCode::OK);
    assert_eq!(open_count(&harness).await, 0);
    assert_retired(
        &status_of(&harness, proposed).await,
        &status_of(&harness, waiting).await,
        &status_of(&harness, running).await,
    );
    let store = harness.runtime.as_ref().unwrap().store.clone();
    let report = store.verify_audit(VerifyScope::Full).await.unwrap();
    assert!(
        matches!(report.chained, ChainStatus::Intact { .. }),
        "the audit chain verifies: {:?}",
        report.chained
    );
    harness.stop().await;
}

#[tokio::test]
async fn readiness_ignores_the_removed_action_service() {
    for extra in ["", "[orchestration]\nenabled = false"] {
        let harness = Harness::new(extra, start()).await;
        let (status, body) = readyz(&harness).await;
        assert_eq!(status, StatusCode::OK, "{extra:?}: {body}");
        assert!(body.contains(r#""ready":true"#), "{extra:?}: {body}");
        assert!(!body.contains("action_service"), "{extra:?}: {body}");
        harness.stop().await;
    }
}

#[tokio::test]
async fn orchestration_off_still_retires_legacy_rows_and_executes_nothing() {
    let (mut harness, [proposed, waiting, running]) =
        restarted_with_open_actions("[orchestration]\nenabled = false").await;
    harness.start_schedulers();
    assert!(harness.runtime.as_ref().unwrap().operations_task.is_none());

    // The minute scheduler runs against the fake clock; nothing it does touches the
    // retired rows.
    guard(async {
        for _ in 0..50 {
            harness.clock.advance(SignedDuration::from_mins(1));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;

    assert_eq!(open_count(&harness).await, 0);
    assert_retired(
        &status_of(&harness, proposed).await,
        &status_of(&harness, waiting).await,
        &status_of(&harness, running).await,
    );
    let store = harness.runtime.as_ref().unwrap().store.clone();
    let audit = AuditLog::list(&*store, &AuditFilter::default(), &Page::first(200))
        .await
        .unwrap()
        .items;
    assert!(
        !audit.iter().any(|event| matches!(
            event.kind.as_str(),
            "action.approved" | "action.dispatched" | "action.succeeded"
        )),
        "nothing was approved, dispatched or run: {:?}",
        audit.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    harness.stop().await;
}
