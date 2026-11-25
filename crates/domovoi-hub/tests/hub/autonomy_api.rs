//! The autonomy endpoints: who may pause, resume, reset a breaker and resolve an unknown outcome,
//! and what each needs.

use axum::http::{Method, StatusCode};
use serde_json::json;

use crate::api_harness::{PASSWORD, api};

async fn state(
    a: &crate::api_harness::Api,
    login: &crate::api_harness::Login,
) -> serde_json::Value {
    let reply = a.get("/api/v1/policy/autonomy", login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

#[tokio::test]
async fn autonomy_starts_not_paused_with_no_suspended_rule() {
    let a = api().await;
    let login = a.admin().await;
    let body = state(&a, &login).await;
    assert_eq!(body["paused"], false);
    assert_eq!(body["suspended_rules"], json!([]));
}

#[tokio::test]
async fn pause_requires_a_session_and_csrf_and_needs_no_password() {
    let a = api().await;
    let login = a.admin().await;
    // No session.
    let anonymous = a
        .send(
            Method::POST,
            "/api/v1/policy/autonomy/pause",
            None,
            Some(json!({ "reason": "x" })),
        )
        .await;
    assert_eq!(
        anonymous.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        anonymous.text
    );
    // A session and the CSRF header pause it, without a password.
    let paused = a
        .post(
            "/api/v1/policy/autonomy/pause",
            &login,
            json!({ "reason": "maintenance window" }),
        )
        .await;
    assert_eq!(paused.status, StatusCode::OK, "{}", paused.text);
    assert_eq!(paused.body["paused"], true);
    assert_eq!(paused.body["reason"], "maintenance window");
    // An empty or over-long reason is refused.
    for reason in ["  ".to_owned(), "x".repeat(281)] {
        let bad = a
            .post(
                "/api/v1/policy/autonomy/pause",
                &login,
                json!({ "reason": reason }),
            )
            .await;
        assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.text);
    }
}

#[tokio::test]
async fn resume_requires_the_password_and_counts_toward_the_lockout() {
    let a = api().await;
    let login = a.admin().await;
    a.post(
        "/api/v1/policy/autonomy/pause",
        &login,
        json!({ "reason": "maintenance" }),
    )
    .await;
    let missing = a
        .post("/api/v1/policy/autonomy/resume", &login, json!({}))
        .await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    let wrong = a
        .post(
            "/api/v1/policy/autonomy/resume",
            &login,
            json!({ "password": "not the password" }),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN, "{}", wrong.text);
    assert_eq!(
        state(&a, &login).await["paused"],
        true,
        "a refused resume changes nothing"
    );
    let resumed = a
        .post(
            "/api/v1/policy/autonomy/resume",
            &login,
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(resumed.status, StatusCode::OK, "{}", resumed.text);
    assert_eq!(resumed.body["paused"], false);
}

#[tokio::test]
async fn a_refused_password_writes_no_audit_row_and_a_good_one_writes_one() {
    use domovoi_core::ports::{AuditFilter, Page};
    let a = api().await;
    let login = a.admin().await;
    a.post(
        "/api/v1/policy/autonomy/pause",
        &login,
        json!({ "reason": "maintenance" }),
    )
    .await;
    a.post(
        "/api/v1/policy/autonomy/resume",
        &login,
        json!({ "password": "wrong" }),
    )
    .await;
    let kinds = |a: &crate::api_harness::Api| {
        let audit = a.audit.clone();
        async move {
            audit
                .list(&AuditFilter::default(), &Page::first(200))
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>()
        }
    };
    assert!(!kinds(&a).await.contains(&"autonomy.resumed".to_owned()));
    a.post(
        "/api/v1/policy/autonomy/resume",
        &login,
        json!({ "password": PASSWORD }),
    )
    .await;
    let all = kinds(&a).await;
    assert!(all.contains(&"autonomy.paused".to_owned()), "{all:?}");
    assert!(all.contains(&"autonomy.resumed".to_owned()), "{all:?}");
}

#[tokio::test]
async fn reset_rule_requires_the_password_and_a_valid_rule_id() {
    let a = api().await;
    let login = a.admin().await;
    let missing = a
        .post("/api/v1/policy/rules/r-restart/reset", &login, json!({}))
        .await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    let bad_id = a
        .post(
            "/api/v1/policy/rules/bad%20id/reset",
            &login,
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST, "{}", bad_id.text);
    let reset = a
        .post(
            "/api/v1/policy/rules/r-restart/reset",
            &login,
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(reset.status, StatusCode::OK, "{}", reset.text);
}

#[tokio::test]
async fn resolve_unknown_requires_the_password_and_a_step_with_an_unknown_outcome() {
    let a = api().await;
    let login = a.admin().await;
    let step = "01900000-0000-7000-8000-000000000001";
    let uri = format!(
        "/api/v1/operations/01900000-0000-7000-8000-000000000002/steps/{step}/resolve-unknown"
    );
    let missing = a
        .post(&uri, &login, json!({ "note": "checked the host" }))
        .await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    // With the password but no such step: not found, never a silent success.
    let unknown = a
        .post(
            &uri,
            &login,
            json!({ "note": "checked", "password": PASSWORD }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{}", unknown.text);
    let short = a
        .post(&uri, &login, json!({ "note": "", "password": PASSWORD }))
        .await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST, "{}", short.text);
}

#[tokio::test]
async fn no_endpoint_accepts_a_client_supplied_permit_or_approval() {
    let a = api().await;
    let login = a.admin().await;
    for (uri, body) in [
        (
            "/api/v1/policy/autonomy/pause",
            json!({ "reason": "x", "permit": { "decision_id": "d" }, "approval": {} }),
        ),
        (
            "/api/v1/policy/autonomy/resume",
            json!({ "password": PASSWORD, "authorization": { "kind": "policy" } }),
        ),
    ] {
        let reply = a.post(uri, &login, body).await;
        // Extra fields are ignored: nothing in the body can become an authorization.
        assert!(
            reply.status == StatusCode::OK || reply.status.is_client_error(),
            "{uri}: {}",
            reply.text
        );
    }
    let routes = a.get("/api/v1/policy/autonomy", &login).await;
    assert!(!routes.text.contains("permit"));
}

#[test]
fn execution_dto_shows_authorization_attribution() {
    use domovoi_core::{
        Authorization, CheckKey, ContractId, ExecutionIntent, ExecutionRecord, OperationId,
        PlanHash, PolicyPermit, PolicyPermitParts, Risk, StepId, TargetKey,
    };
    use domovoi_hub::api::dto_operations::AuthorizationDto;
    use domovoi_testkit::build::T0;
    use jiff::SignedDuration;
    use std::collections::BTreeMap;

    let intent = ExecutionIntent {
        contract: ContractId::parse("unit.restart").unwrap(),
        contract_version: 1,
        host: domovoi_core::HostId::new(),
        check: CheckKey::parse("unit.failed").unwrap(),
        target: Some(TargetKey::parse("example.service").unwrap()),
        args: BTreeMap::from([("unit".to_owned(), "example.service".to_owned())]),
        retry_of: None,
    };
    let plan: PlanHash = "ab".repeat(32).parse().unwrap();
    let mut row = ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        plan,
        T0 + SignedDuration::from_hours(1),
        Risk::Write,
        T0,
    );
    assert_eq!(AuthorizationDto::of(&row), None, "nobody authorized it yet");
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "decision-9",
        rule_ids: vec!["rule-b".to_owned(), "rule-a".to_owned()],
        policy_version: "4",
        plan_hash: plan,
        issued_at: T0,
        expires_at: T0 + SignedDuration::from_mins(5),
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .unwrap();
    row.authorize_by_permit(&Authorization::policy(permit), "doc".to_owned(), T0)
        .unwrap();
    let dto = AuthorizationDto::of(&row).expect("a policy authorization");
    assert_eq!(dto.kind, "policy");
    assert!(dto.user.is_none() && dto.channel.is_none());
    let permit = dto.permit.expect("the permit is shown");
    assert_eq!(permit.decision_id, "decision-9");
    assert_eq!(permit.rules, ["rule-a", "rule-b"]);
    assert_eq!(permit.policy_version, "4");
}

#[test]
fn openapi_lists_the_autonomy_routes() {
    let spec: serde_json::Value =
        serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    let paths = spec["paths"].as_object().unwrap();
    for (path, method) in [
        ("/api/v1/policy/autonomy", "get"),
        ("/api/v1/policy/autonomy/pause", "post"),
        ("/api/v1/policy/autonomy/resume", "post"),
        ("/api/v1/policy/rules/{rule}/reset", "post"),
        (
            "/api/v1/operations/{id}/steps/{step}/resolve-unknown",
            "post",
        ),
    ] {
        assert!(
            paths[path][method].is_object(),
            "{method} {path} is missing"
        );
    }
}
