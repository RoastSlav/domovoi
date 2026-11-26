//! The four operation controls: cancel, pause, resume and answer, over the operation
//! harness with orchestration on.

use axum::http::{Method, StatusCode};
use domovoi_core::{OperationId, StepId, StepIntent, StepKind, StepOutcome};
use domovoi_engine::operations::NewStep;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api, api_with_operations};

const SECRET: &str = "sk-ant-test-0123456789abcdef";

/// An operation with one open question, and the question's step id.
async fn with_question(a: &Api) -> (OperationId, StepId) {
    let op = a
        .create_operation("Investigate: disk full on nas", true)
        .await;
    let question = StepId::new();
    a.ops()
        .propose(
            op,
            vec![NewStep {
                id: question,
                kind: StepKind::Question,
                phase: "ask".to_owned(),
                parent: None,
                intent: StepIntent::Question {
                    text: "Is /srv the backup disk?".to_owned(),
                    expires_at: a.now() + SignedDuration::from_hours(24),
                },
            }],
            Vec::new(),
        )
        .await
        .unwrap();
    (op, question)
}

async fn post(
    a: &Api,
    login: &Login,
    path: &str,
    body: Option<Value>,
) -> crate::api_harness::Reply {
    a.send(
        Method::POST,
        &format!("/api/v1/operations/{path}"),
        Some(login),
        body,
    )
    .await
}

async fn status_of(a: &Api, op: OperationId) -> String {
    let stored = a
        .state
        .deps()
        .operation_repo
        .get(op)
        .await
        .unwrap()
        .unwrap();
    stored.value.status.name().to_owned()
}

#[tokio::test]
async fn cancel_route_cancels_and_records_the_person() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, question) = with_question(&a).await;
    let before = a.head_seq().await;
    let reply = post(
        &a,
        &login,
        &format!("{op}/cancel"),
        Some(json!({ "reason": "wrong host" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["operation"]["status"], "cancelled");
    assert!(reply.body["as_of_seq"].as_u64().unwrap() > before);
    let snapshot = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    let step = snapshot.step(question).unwrap();
    assert_eq!(step.progress.outcome, StepOutcome::Cancelled);
    let audit = a.audit_events("operation.cancelled").await;
    assert_eq!(audit.len(), 1);
    assert!(audit[0].actor.starts_with("user:"));
}

#[tokio::test]
async fn cancel_twice_writes_once() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    for _ in 0..2 {
        let reply = post(&a, &login, &format!("{op}/cancel"), Some(json!({}))).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    }
    assert_eq!(a.audit_events("operation.cancelled").await.len(), 1);
}

#[tokio::test]
async fn cancel_reason_is_bounded() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    let long = "x".repeat(501);
    let reply = post(
        &a,
        &login,
        &format!("{op}/cancel"),
        Some(json!({ "reason": long })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        reply.body["message"],
        "the reason must have at most 500 characters"
    );
    assert_eq!(status_of(&a, op).await, "waiting_user");
}

#[tokio::test]
async fn pause_route_pauses_with_a_manual_reason_and_resume_needs_no_extension() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    let reply = post(&a, &login, &format!("{op}/pause"), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["operation"]["status"], "paused");
    assert_eq!(reply.body["operation"]["pause_reason"]["type"], "manual");
    let again = post(&a, &login, &format!("{op}/pause"), None).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(a.audit_events("operation.paused").await.len(), 1);

    let resumed = post(&a, &login, &format!("{op}/resume"), Some(json!({}))).await;
    assert_eq!(resumed.status, StatusCode::OK, "{}", resumed.text);
    assert_ne!(resumed.body["operation"]["status"], "paused");
    assert_eq!(a.audit_events("operation.resumed").await.len(), 1);
}

#[tokio::test]
async fn resume_of_an_operation_that_is_not_paused_is_a_conflict() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    let reply = post(&a, &login, &format!("{op}/resume"), Some(json!({}))).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.body["message"], "the operation is not paused");
}

#[tokio::test]
async fn resume_validates_the_extension() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    let entry = |kind: &str, limit: u64| json!({ "kind": kind, "limit": limit });
    let many: Vec<Value> = (0..8).map(|_| entry("cycles", 1)).collect();
    let cases = [
        (vec![entry("nope", 1)], "unknown limit `nope`"),
        (
            vec![entry("tokens", 1), entry("tokens", 2)],
            "each limit can be raised once",
        ),
        (many, "at most 7 limits can be raised at once"),
        (
            vec![entry("executions", 0)],
            "the limit for `executions` must be between 1 and 10000",
        ),
        (
            vec![entry("executions", 10_001)],
            "the limit for `executions` must be between 1 and 10000",
        ),
        (
            vec![entry("tokens", 10_000_000_001)],
            "the limit for `tokens` must be between 1 and 10000000000",
        ),
    ];
    for (extend, message) in cases {
        let reply = post(
            &a,
            &login,
            &format!("{op}/resume"),
            Some(json!({ "extend": extend })),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
        assert_eq!(reply.body["message"], message);
    }
}

#[tokio::test]
async fn answer_route_finishes_the_question_with_a_note_and_audits_without_the_text() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, question) = with_question(&a).await;
    let text = format!("  That is the backup disk; token {SECRET}  ");
    let reply = post(
        &a,
        &login,
        &format!("{op}/steps/{question}/answer"),
        Some(json!({ "text": text })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_ne!(reply.body["operation"]["status"], "waiting_user");
    let snapshot = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.step(question).unwrap().progress.outcome,
        StepOutcome::Succeeded
    );
    let note = snapshot
        .steps
        .iter()
        .find(|s| s.step.kind == StepKind::Note)
        .expect("an answer is a note");
    let StepIntent::Note { text: stored, .. } = &note.step.intent else {
        panic!("a note");
    };
    assert!(!stored.contains(SECRET), "{stored}");
    assert!(
        stored.starts_with("That is the backup disk"),
        "trimmed: {stored:?}"
    );
    let audit = a.audit_events("operation.answered").await;
    assert_eq!(audit.len(), 1);
    assert!(audit[0].actor.starts_with("user:"));
    assert!(audit[0].detail["operation"].is_string() && audit[0].detail["step"].is_string());
    assert!(!audit[0].detail.to_string().contains("backup disk"));
}

#[tokio::test]
async fn answer_refusals() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, question) = with_question(&a).await;
    let path = |step: &dyn std::fmt::Display| format!("{op}/steps/{step}/answer");
    for bad in ["", "   ", &"y".repeat(2001)] {
        let reply = post(&a, &login, &path(&question), Some(json!({ "text": bad }))).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            reply.body["message"],
            "the answer must have 1 to 2000 characters"
        );
    }
    let unknown = post(
        &a,
        &login,
        &path(&StepId::new()),
        Some(json!({ "text": "x" })),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let paused = {
        post(&a, &login, &format!("{op}/pause"), None).await;
        post(&a, &login, &path(&question), Some(json!({ "text": "x" }))).await
    };
    assert_eq!(paused.status, StatusCode::CONFLICT);
    assert_eq!(
        paused.body["message"],
        "the operation is paused; resume it before answering"
    );
    post(&a, &login, &format!("{op}/resume"), Some(json!({}))).await;
    let ok = post(&a, &login, &path(&question), Some(json!({ "text": "x" }))).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let again = post(&a, &login, &path(&question), Some(json!({ "text": "x" }))).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(
        again.body["message"],
        "this question is not waiting for an answer"
    );
}

#[tokio::test]
async fn control_routes_need_a_session_and_the_csrf_header() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, question) = with_question(&a).await;
    let paths = [
        format!("{op}/cancel"),
        format!("{op}/pause"),
        format!("{op}/resume"),
        format!("{op}/steps/{question}/answer"),
    ];
    for path in &paths {
        let uri = format!("/api/v1/operations/{path}");
        let anonymous = a.send(Method::POST, &uri, None, Some(json!({}))).await;
        assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED, "{path}");
        let no_csrf = Login {
            cookie: login.cookie.clone(),
            csrf: String::new(),
        };
        let reply = a
            .send(Method::POST, &uri, Some(&no_csrf), Some(json!({})))
            .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(status_of(&a, op).await, "waiting_user");
}

#[tokio::test]
async fn control_routes_answer_503_when_orchestration_is_off() {
    let a = api().await;
    let login = a.admin().await;
    let (op, step) = (OperationId::new(), StepId::new());
    for path in [
        format!("{op}/cancel"),
        format!("{op}/pause"),
        format!("{op}/resume"),
        format!("{op}/steps/{step}/answer"),
    ] {
        let reply = post(&a, &login, &path, Some(json!({ "text": "x" }))).await;
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(reply.body["message"], "orchestration is not enabled");
    }
}

#[tokio::test]
async fn control_responses_read_the_journal_head_after_the_command() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, _) = with_question(&a).await;
    let reply = post(&a, &login, &format!("{op}/pause"), None).await;
    let head = a.head_seq().await;
    assert_eq!(reply.body["as_of_seq"].as_u64().unwrap(), head);
    assert_eq!(reply.body["operation"]["status"], "paused");
}

#[tokio::test]
async fn openapi_lists_the_four_control_routes() {
    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    let paths = [
        ("/api/v1/operations/{id}/cancel", "cancel_operation"),
        ("/api/v1/operations/{id}/pause", "pause_operation"),
        ("/api/v1/operations/{id}/resume", "resume_operation"),
        (
            "/api/v1/operations/{id}/steps/{step}/answer",
            "answer_operation_question",
        ),
    ];
    for (path, id) in paths {
        let post = &spec["paths"][path]["post"];
        assert_eq!(post["operationId"], id, "{path}");
        assert_eq!(post["tags"][0], "operations");
        for code in ["200", "401", "404", "409", "503"] {
            assert!(post["responses"][code].is_object(), "{path} {code}");
        }
    }
    assert!(spec["components"]["schemas"]["OperationControlDto"].is_object());
}

#[tokio::test]
async fn find_by_execution_returns_none_for_an_unknown_id_and_the_step_for_an_execution() {
    let a = api_with_operations().await;
    let op = a.create_operation("restart plex", true).await;
    let step = a.propose_restart(op).await;
    let found = a
        .services
        .operations
        .find_by_execution(domovoi_core::ExecutionId::from_step(step))
        .await
        .unwrap()
        .expect("an execution");
    assert_eq!((found.operation, found.step), (op, step));
    assert_eq!(found.incidents, vec![a.incident.id()]);
    let unknown = a
        .services
        .operations
        .find_by_execution(domovoi_core::ExecutionId::new())
        .await
        .unwrap();
    assert!(unknown.is_none());
}
