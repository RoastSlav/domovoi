//! The containment endpoints, in-process over the router: what a host's blocks look like, the
//! owner's refresh, and that nothing asks the helper on its own.

use axum::http::StatusCode;
use domovoi_core::containment::{ContainCall, ContainOutputs, keys};
use domovoi_core::ports::Presence;
use domovoi_core::{AuthorizationKind, HostId, JournalEpoch};
use serde_json::json;
use uuid::Uuid;

use crate::api_harness::{Api, api, t0};

fn connect(a: &Api, host: HostId) {
    a.link.set_presence(
        host,
        Presence::Connected {
            since: t0(),
            version: "0.1.0".to_owned(),
        },
    );
    a.link.set_journal_epoch(
        host,
        JournalEpoch::from_uuid(Uuid::from_u128(host.as_uuid().as_u128())),
    );
}

#[tokio::test]
async fn containment_status_endpoint_works_when_unavailable() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/hosts/nas/containment", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    // No helper, no refresh: the answer says so and holds nothing.
    assert_eq!(reply.body["available"], false);
    assert_eq!(reply.body["gate"]["state"], "unknown");
    assert_eq!(reply.body["drills"]["state"], "unknown");
    assert_eq!(reply.body["blocks"].as_array().map(Vec::len), Some(0));
    assert!(reply.body["observed_at"].is_null());
    let missing = a.get("/api/v1/hosts/nope/containment", &login).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_blocks_is_owner_initiated_only() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, a.nas.id);
    // Reading the status asks nobody.
    let read = a.get("/api/v1/hosts/nas/containment", &login).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.text);
    assert!(a.contain_relay.dispatches().is_empty());

    // The owner's refresh asks once, as a human, and the answer shows in the status.
    a.contain_relay.queue_result(Ok(ContainOutputs(
        [
            (keys::OUT_GATE, "open"),
            (keys::OUT_DRILLS, "passed"),
            (keys::OUT_LAYOUT, "hosts"),
            (keys::OUT_BRIDGE_GAP, "false"),
            (
                keys::OUT_BLOCKS,
                r#"[{"source":"203.0.113.7/32","set":"blocked4","remaining_s":300,"phase":"confirmed"}]"#,
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    )));
    let reply = a
        .post("/api/v1/hosts/nas/containment/refresh", &login, json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["gate"]["state"], "open");
    assert_eq!(reply.body["drills"]["state"], "passed");
    assert_eq!(reply.body["bridge_gap"], false);
    let sent = a.contain_relay.dispatches();
    assert_eq!(sent.len(), 1);
    assert!(matches!(sent[0].call(), ContainCall::List));
    assert_eq!(sent[0].authorization(), AuthorizationKind::Human);
    assert!(sent[0].expected_safety().is_none());
    let audits = a.audit_events("containment.listed").await;
    assert_eq!(audits.len(), 1);
    // A later read does not ask again.
    let again = a.get("/api/v1/hosts/nas/containment", &login).await;
    assert_eq!(again.body["drills"]["state"], "passed");
    assert_eq!(a.contain_relay.dispatches().len(), 1);
}

#[tokio::test]
async fn refresh_without_a_session_is_a_conflict_and_without_login_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    // The agent is not connected.
    let reply = a
        .post("/api/v1/hosts/nas/containment/refresh", &login, json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert!(a.contain_relay.dispatches().is_empty());
    let anonymous = a
        .raw(
            axum::http::Request::builder()
                .method("GET")
                .uri("/api/v1/hosts/nas/containment")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn release_of_an_unknown_step_is_not_found() {
    let a = api().await;
    let login = a.admin().await;
    let uri = format!(
        "/api/v1/operations/{}/steps/{}/containment/release",
        domovoi_core::OperationId::new(),
        domovoi_core::StepId::new()
    );
    let reply = a
        .post(
            &uri,
            &login,
            json!({"password": "x", "confirm_source": "203.0.113.7/32"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);
    assert!(a.contain_relay.dispatches().is_empty());
}
