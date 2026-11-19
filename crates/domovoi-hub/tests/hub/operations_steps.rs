//! Approving and denying Execution steps over the API, the legacy action endpoints beside
//! them, and the incident's investigation retry.

use axum::http::StatusCode;
use domovoi_core::ports::{AggregateRef, EffectStatus};
use domovoi_core::{ApprovalChannel, OperationId, Risk, StepId};
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api, api_with_operations, api_with_triage_and_operations};

/// The stored v2 plan hash of Execution step `step`.
async fn hash_of(a: &Api, op: OperationId, step: StepId) -> String {
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    stored
        .execution_of(step)
        .unwrap()
        .value
        .plan_hash
        .to_string()
}

fn approve_uri(op: OperationId, step: StepId) -> String {
    format!("/api/v1/operations/{op}/steps/{step}/approve")
}

async fn stored_state(a: &Api, login: &Login, op: OperationId, step: StepId) -> String {
    let reply = a.get(&format!("/api/v1/operations/{op}"), login).await;
    reply.body["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["step"] == step.to_string())
        .unwrap()["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn dispatches_of(a: &Api, op: OperationId) -> usize {
    let mut found = 0;
    for status in [
        EffectStatus::Pending,
        EffectStatus::Claimed,
        EffectStatus::Done,
    ] {
        found += a
            .effects(status)
            .await
            .iter()
            .filter(|e| e.kind == "dispatch" && e.aggregate == AggregateRef::operation(op))
            .count();
    }
    found
}

#[tokio::test]
async fn approve_step_with_the_shown_hash_authorizes_and_dispatches() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let step = a.propose_restart(op).await;
    assert_eq!(dispatches_of(&a, op).await, 0);
    let hash = hash_of(&a, op, step).await;

    let reply = a
        .post(&approve_uri(op, step), &login, json!({ "plan_hash": hash }))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["state"], "authorized");
    assert_eq!(reply.body["plan_hash"], hash.as_str());
    let user = a.get("/api/v1/session", &login).await.body["user"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(reply.body["approved_by"], format!("user:{user}"));

    assert_eq!(
        dispatches_of(&a, op).await,
        1,
        "a dispatch effect was queued"
    );
    let audit = a.audit_events("execution.approved").await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].actor, format!("user:{user}"));
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    let approval = stored.execution_of(step).unwrap().value.approval().unwrap();
    assert_eq!(
        approval.approval().channel(),
        &ApprovalChannel::Web {
            typed_confirmation: false
        }
    );
}

#[tokio::test]
async fn approve_step_without_a_hash_is_a_bad_request() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let step = a.propose_restart(op).await;

    for body in [
        json!({}),
        json!({ "plan_hash": null }),
        json!({ "plan_hash": "not-hex" }),
        json!({ "plan_hash": "ab" }),
    ] {
        let reply = a.post(&approve_uri(op, step), &login, body.clone()).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            reply.text
        );
    }
    assert_eq!(
        stored_state(&a, &login, op, step).await,
        "awaiting_approval"
    );
    assert_eq!(dispatches_of(&a, op).await, 0);
    assert!(a.audit_events("execution.approved").await.is_empty());
}

#[tokio::test]
async fn approve_step_with_another_hash_is_a_conflict() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let step = a.propose_restart(op).await;
    let other = "0".repeat(64);
    assert_ne!(other, hash_of(&a, op, step).await);

    let reply = a
        .post(
            &approve_uri(op, step),
            &login,
            json!({ "plan_hash": other }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap()
            .contains("the plan changed"),
        "{}",
        reply.text
    );
    assert_eq!(
        stored_state(&a, &login, op, step).await,
        "awaiting_approval"
    );
    assert_eq!(dispatches_of(&a, op).await, 0);
}

#[tokio::test]
async fn approve_destructive_step_needs_the_skill_name() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("prune", false).await;
    let step = a.propose_run(op, "prune_images", &[]).await;
    let hash = hash_of(&a, op, step).await;

    for body in [
        json!({ "plan_hash": hash }),
        json!({ "plan_hash": hash, "confirm_text": "prune" }),
        json!({ "plan_hash": hash, "confirm_text": "Prune_images" }),
    ] {
        let reply = a.post(&approve_uri(op, step), &login, body.clone()).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{body}: {}",
            reply.text
        );
    }
    assert_eq!(
        stored_state(&a, &login, op, step).await,
        "awaiting_approval"
    );

    let reply = a
        .post(
            &approve_uri(op, step),
            &login,
            json!({ "plan_hash": hash, "confirm_text": "prune_images" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    let approval = stored.execution_of(step).unwrap().value.approval().unwrap();
    assert_eq!(
        approval.approval().channel(),
        &ApprovalChannel::Web {
            typed_confirmation: true
        }
    );
}

#[tokio::test]
async fn approve_step_for_a_skill_gone_from_the_host_needs_the_skill_name() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let step = a.propose_restart(op).await;
    let hash = hash_of(&a, op, step).await;
    a.link.set_skills(a.nas.id, Vec::new());

    let shown = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    assert!(shown.body["executions"][0]["risk"].is_null());
    assert_eq!(shown.body["executions"][0]["confirm_required"], true);

    let unconfirmed = a
        .post(&approve_uri(op, step), &login, json!({ "plan_hash": hash }))
        .await;
    assert_eq!(
        unconfirmed.status,
        StatusCode::FORBIDDEN,
        "{}",
        unconfirmed.text
    );
    assert_eq!(dispatches_of(&a, op).await, 0);

    let confirmed = a
        .post(
            &approve_uri(op, step),
            &login,
            json!({ "plan_hash": hash, "confirm_text": "restart_container" }),
        )
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.text);
}

#[tokio::test]
async fn approve_expired_and_double_approve_are_conflicts() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let first = a.propose_restart(op).await;
    let second = a
        .propose_run(op, "restart_container", &[("container", "jellyfin")])
        .await;
    let (first_hash, second_hash) = (hash_of(&a, op, first).await, hash_of(&a, op, second).await);

    let approved = a
        .post(
            &approve_uri(op, first),
            &login,
            json!({ "plan_hash": first_hash }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);
    let again = a
        .post(
            &approve_uri(op, first),
            &login,
            json!({ "plan_hash": first_hash }),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text);

    // Past the 24 hour window, in steps shorter than the session's idle limit.
    for _ in 0..5 {
        a.advance(SignedDuration::from_hours(6));
        assert_eq!(
            a.get("/api/v1/session", &login).await.status,
            StatusCode::OK
        );
    }
    let late = a
        .post(
            &approve_uri(op, second),
            &login,
            json!({ "plan_hash": second_hash }),
        )
        .await;
    assert_eq!(late.status, StatusCode::CONFLICT, "{}", late.text);
    assert_eq!(
        dispatches_of(&a, op).await,
        1,
        "only the first was dispatched"
    );
}

#[tokio::test]
async fn deny_step_refuses_the_execution() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("restart plex", false).await;
    let step = a.propose_restart(op).await;

    let reply = a
        .post(
            &format!("/api/v1/operations/{op}/steps/{step}/deny"),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["state"], "refused");
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    let approval_step = reply.body["approval_step"].as_str().unwrap();
    let record = stored
        .steps
        .iter()
        .find(|r| r.step.id.to_string() == approval_step)
        .unwrap();
    assert_eq!(record.progress.outcome.as_str(), "refused");
    assert_eq!(
        stored.step(step).unwrap().progress.outcome.as_str(),
        "refused"
    );
    let audit = a.audit_events("execution.denied").await;
    assert_eq!(audit.len(), 1);
    assert!(audit[0].actor.starts_with("user:"));
    assert_eq!(dispatches_of(&a, op).await, 0);

    let hash = hash_of(&a, op, step).await;
    let late = a
        .post(&approve_uri(op, step), &login, json!({ "plan_hash": hash }))
        .await;
    assert_eq!(
        late.status,
        StatusCode::CONFLICT,
        "a denied step cannot be approved"
    );
}

#[tokio::test]
async fn legacy_action_endpoints_still_work_and_are_labelled() {
    let on = api_with_operations().await;
    let login = on.admin().await;
    let action = on.propose(Risk::Write).await;
    let listed = on.get("/api/v1/actions", &login).await;
    assert_eq!(listed.body["items"][0]["legacy"], true, "{}", listed.text);
    let approved = on
        .post(
            &format!("/api/v1/actions/{}/approve", action.id()),
            &login,
            json!({ "plan_hash": action.plan_hash_for_approval().to_string() }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);
    assert_eq!(approved.body["status"], "approved");
    assert_eq!(approved.body["legacy"], true);
    let mut after = approved.body["status"].clone();
    for _ in 0..10_000 {
        let now = on.get("/api/v1/actions?status=approved", &login).await;
        if now.body["items"].as_array().unwrap().is_empty() {
            after = Value::Null;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        after,
        Value::Null,
        "the legacy executor took the approved action"
    );

    let off = api().await;
    let login = off.admin().await;
    let action = off.propose(Risk::Write).await;
    let listed = off.get("/api/v1/actions", &login).await;
    assert_eq!(listed.body["items"][0]["legacy"], false, "{}", listed.text);
    let approved = off
        .post(
            &format!("/api/v1/actions/{}/approve", action.id()),
            &login,
            json!({ "plan_hash": action.plan_hash_for_approval().to_string() }),
        )
        .await;
    assert_eq!(approved.body["legacy"], false);
}

#[tokio::test]
async fn listing_legacy_actions_creates_no_operations_rows() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    a.propose(Risk::Write).await;
    a.propose(Risk::Destructive).await;
    let head = a.head_seq().await;

    let actions = a.get("/api/v1/actions", &login).await;
    assert_eq!(actions.body["items"].as_array().unwrap().len(), 2);
    let detail = a
        .get(&format!("/api/v1/incidents/{}", a.incident.id()), &login)
        .await;
    assert_eq!(detail.body["actions"].as_array().unwrap().len(), 2);
    assert_eq!(detail.body["operations"], json!([]));
    let operations = a.get("/api/v1/operations", &login).await;
    assert_eq!(operations.body["items"], json!([]));
    assert_eq!(a.head_seq().await, head, "reading wrote nothing");
}

#[tokio::test]
async fn retry_triage_runs_an_investigation_when_enabled() {
    let a = api_with_triage_and_operations().await;
    let login = a.admin().await;
    let first = a.ops().investigate(&a.incident, false).await.unwrap();
    let uri = format!("/api/v1/incidents/{}/triage", a.incident.id());

    let again = a.post(&uri, &login, json!({})).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text);
    assert!(
        again.body["message"]
            .as_str()
            .unwrap()
            .contains("already running")
    );
    let operations: Value = a.get("/api/v1/operations", &login).await.body;
    let items = operations["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "no second investigation was started");
    assert_eq!(items[0]["id"], first.to_string());
}
