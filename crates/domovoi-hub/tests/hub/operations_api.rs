//! The operation read endpoints: the list, the detail with its executions, the trace, one
//! step, the incident page's operations, and what each answers while orchestration is off.

use axum::http::{Method, StatusCode};
use domovoi_core::{Actor, OperationTrace, StepIntent, StepKind, StepOutcome, ports::HostRepo};
use domovoi_engine::operations::{NewStep, StepFinish};
use domovoi_hub::api::dto_operations::OperationTraceDto;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Login, api, api_with_operations};

const GOLDEN: [(&str, &str); 5] = [
    (
        "failed-verification-new-cycle",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operation-trace/failed-verification-new-cycle.json"
        )),
    ),
    (
        "outcome-unknown-paused",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operation-trace/outcome-unknown-paused.json"
        )),
    ),
    (
        "question-and-budget-pause",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operation-trace/question-and-budget-pause.json"
        )),
    ),
    (
        "verified-success",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operation-trace/verified-success.json"
        )),
    ),
    (
        "waiting-approval",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operation-trace/waiting-approval.json"
        )),
    ),
];

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn operations_list_filters_and_pages() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let other = domovoi_testkit::build::host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &other).await.unwrap();

    let first = a.create_operation("first", true).await;
    a.advance(SignedDuration::from_secs(10));
    let second = a.create_operation("second", false).await;
    a.advance(SignedDuration::from_secs(10));
    let third = a.create_operation("third", false).await;
    a.ops()
        .cancel(second, Actor::System("tests".to_owned()), "no".to_owned())
        .await
        .unwrap();
    a.propose_restart(third).await;
    let (first, second, third) = (first.to_string(), second.to_string(), third.to_string());

    let all = a.get("/api/v1/operations", &login).await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.text);
    assert_eq!(
        ids(&all.body),
        [third.as_str(), second.as_str(), first.as_str()],
        "newest first"
    );
    assert_eq!(all.body["items"][0]["kind"], "investigate");

    let cancelled = a.get("/api/v1/operations?status=cancelled", &login).await;
    assert_eq!(ids(&cancelled.body), [second.as_str()]);
    let waiting = a
        .get("/api/v1/operations?status=waiting_approval", &login)
        .await;
    assert_eq!(ids(&waiting.body), [third.as_str()]);
    assert_eq!(waiting.body["items"][0]["waiting_for"], json!(["approval"]));

    let about = format!("/api/v1/operations?incident={}", a.incident.id());
    assert_eq!(ids(&a.get(&about, &login).await.body), [first.as_str()]);
    let on_nas = a.get("/api/v1/operations?host=nas", &login).await;
    assert_eq!(on_nas.body["items"].as_array().unwrap().len(), 3);
    let on_media = a.get("/api/v1/operations?host=media", &login).await;
    assert!(on_media.body["items"].as_array().unwrap().is_empty());
    let kind = a.get("/api/v1/operations?kind=investigate", &login).await;
    assert_eq!(kind.body["items"].as_array().unwrap().len(), 3);

    let unknown_status = a.get("/api/v1/operations?status=sleeping", &login).await;
    assert_eq!(unknown_status.status, StatusCode::BAD_REQUEST);
    let unknown_kind = a.get("/api/v1/operations?kind=nap", &login).await;
    assert_eq!(unknown_kind.status, StatusCode::BAD_REQUEST);
    let unknown_host = a.get("/api/v1/operations?host=nowhere", &login).await;
    assert_eq!(unknown_host.status, StatusCode::NOT_FOUND);

    let page_one = a.get("/api/v1/operations?limit=2", &login).await;
    assert_eq!(ids(&page_one.body), [third.as_str(), second.as_str()]);
    let cursor = page_one.body["next_cursor"].as_str().unwrap();
    let page_two = a
        .get(
            &format!("/api/v1/operations?limit=2&cursor={cursor}"),
            &login,
        )
        .await;
    assert_eq!(ids(&page_two.body), [first.as_str()]);
    assert!(page_two.body["next_cursor"].is_null());
}

#[tokio::test]
async fn operations_endpoints_need_a_session() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let step = a.propose_restart(op).await;
    let base = format!("/api/v1/operations/{op}");
    for uri in [
        "/api/v1/operations".to_owned(),
        base.clone(),
        format!("{base}/trace"),
        format!("{base}/steps/{step}"),
    ] {
        let reply = a.send(Method::GET, &uri, None, None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    for action in ["approve", "deny"] {
        let uri = format!("{base}/steps/{step}/{action}");
        let anonymous = a.send(Method::POST, &uri, None, Some(json!({}))).await;
        assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED, "{uri}");
        let no_csrf = Login {
            cookie: login.cookie.clone(),
            csrf: String::new(),
        };
        let forged = a
            .send(Method::POST, &uri, Some(&no_csrf), Some(json!({})))
            .await;
        assert_eq!(
            forged.status,
            StatusCode::FORBIDDEN,
            "{uri}: {}",
            forged.text
        );
    }
    let snapshot = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    assert!(
        snapshot
            .execution_of(step)
            .unwrap()
            .value
            .approval()
            .is_none()
    );
}

#[test]
fn operation_trace_json_equals_the_golden_fixtures() {
    for (name, text) in GOLDEN {
        let expected: Value = serde_json::from_str(text).unwrap();
        let trace: OperationTrace =
            serde_json::from_str(text).unwrap_or_else(|err| panic!("{name} is not a trace: {err}"));
        let got = serde_json::to_value(OperationTraceDto::from(&trace)).unwrap();
        assert_eq!(got, expected, "{name}");
    }
}

#[tokio::test]
async fn operation_detail_lists_executions_with_the_v2_plan_hash() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let restart = a.propose_restart(op).await;
    let prune = a.propose_run(op, "prune_images", &[]).await;
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();

    let reply = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let executions = reply.body["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 2);
    let by_step = |step: &str| {
        executions
            .iter()
            .find(|e| e["step"] == step)
            .unwrap_or_else(|| panic!("no execution for {step}"))
    };
    for step in [restart, prune] {
        let shown = by_step(&step.to_string());
        let hash = shown["plan_hash"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(
            hash,
            stored
                .execution_of(step)
                .unwrap()
                .value
                .plan_hash
                .to_string()
        );
        assert_eq!(shown["plan_binding"], "v2");
        assert_eq!(shown["state"], "awaiting_approval");
        assert!(shown["approval_step"].is_string());
    }
    let write = by_step(&restart.to_string());
    assert_eq!(write["contract"], "skill.restart_container");
    assert_eq!(write["risk"], "write");
    assert_eq!(write["confirm_required"], false);
    assert_eq!(write["args"], json!({ "container": "plex" }));
    let destructive = by_step(&prune.to_string());
    assert_eq!(destructive["risk"], "destructive");
    assert_eq!(destructive["confirm_required"], true);

    // The risk comes from the host's skills now: a skill that left is unknown, and an
    // unknown risk asks for the name too.
    a.link
        .set_skills(a.nas.id, vec![crate::ops_harness::prune_skill()]);
    let after = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    let gone = after.body["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["step"] == restart.to_string())
        .unwrap();
    assert!(gone["risk"].is_null());
    assert_eq!(gone["confirm_required"], true);
}

#[tokio::test]
async fn operation_reads_carry_as_of_seq_read_before_loading() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    a.propose_restart(op).await;

    let head = a.head_seq().await;
    assert!(head > 0, "the journal has entries");
    let list = a.get("/api/v1/operations", &login).await;
    assert_eq!(list.body["as_of_seq"], head);
    let detail = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    assert_eq!(detail.body["as_of_seq"], head);
    assert_eq!(detail.body["trace"]["as_of_seq"], head);
    let trace = a
        .get(&format!("/api/v1/operations/{op}/trace"), &login)
        .await;
    assert_eq!(trace.body["as_of_seq"], head);

    a.propose_run(op, "prune_images", &[]).await;
    let moved = a.head_seq().await;
    assert!(moved > head, "a commit moves the head");
    let after = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    assert_eq!(after.body["as_of_seq"], moved);
    assert_eq!(after.body["trace"]["as_of_seq"], moved);
}

#[tokio::test]
async fn step_detail_returns_intent_and_bounded_output() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let step = a.propose_restart(op).await;

    let reply = a
        .get(&format!("/api/v1/operations/{op}/steps/{step}"), &login)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["id"], step.to_string());
    assert_eq!(reply.body["operation_id"], op.to_string());
    assert_eq!(reply.body["kind"], "execution");
    assert_eq!(reply.body["intent"]["contract"], "skill.restart_container");
    assert!(reply.body["output"].is_null());

    let note = domovoi_core::StepId::new();
    a.ops()
        .propose(
            op,
            vec![NewStep {
                id: note,
                kind: StepKind::ReasoningTurn,
                phase: "investigate".to_owned(),
                parent: None,
                intent: StepIntent::ReasoningTurn {
                    context_refs: vec!["incident".to_owned()],
                },
            }],
            Vec::new(),
        )
        .await
        .unwrap();
    let output = format!("key=sk-ant-test-0123456789abcdef {}", "x".repeat(40_000));
    a.ops()
        .step_finished(
            op,
            note,
            StepFinish {
                outcome: StepOutcome::Succeeded,
                summary: Some("read".to_owned()),
                reason: None,
                output: Some(output),
                usage: None,
            },
        )
        .await
        .unwrap();
    let long = a
        .get(&format!("/api/v1/operations/{op}/steps/{note}"), &login)
        .await;
    assert_eq!(long.status, StatusCode::OK, "{}", long.text);
    let shown = long.body["output"].as_str().unwrap();
    assert!(shown.len() <= 16 * 1024, "{} bytes", shown.len());
    assert!(!shown.contains("sk-ant-test"), "secrets are redacted");
    assert_eq!(long.body["intent"]["context_refs"], json!(["incident"]));
    assert_eq!(long.body["outcome"], "succeeded");

    let other = a
        .get(
            &format!(
                "/api/v1/operations/{op}/steps/{}",
                domovoi_core::StepId::new()
            ),
            &login,
        )
        .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    let none = a
        .get(
            &format!(
                "/api/v1/operations/{}/trace",
                domovoi_core::OperationId::new()
            ),
            &login,
        )
        .await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn incident_detail_lists_its_operations() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let uri = format!("/api/v1/incidents/{}", a.incident.id());
    let empty = a.get(&uri, &login).await;
    assert_eq!(empty.status, StatusCode::OK, "{}", empty.text);
    assert_eq!(empty.body["operations"], json!([]));

    let older = a.create_operation("older", true).await;
    a.advance(SignedDuration::from_secs(30));
    let newer = a.create_operation("newer", true).await;
    a.create_operation("unrelated", false).await;
    let detail = a.get(&uri, &login).await;
    let listed: Vec<&str> = detail.body["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| op["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [newer.to_string(), older.to_string()]);
    assert_eq!(detail.body["operations"][0]["goal"], "newer");
}

#[tokio::test]
async fn endpoints_answer_503_when_orchestration_is_off() {
    let a = api().await;
    let login = a.admin().await;
    let list = a.get("/api/v1/operations", &login).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text);
    assert_eq!(list.body["items"], json!([]));
    assert!(list.body["next_cursor"].is_null());

    let id = domovoi_core::OperationId::new();
    let step = domovoi_core::StepId::new();
    for uri in [
        format!("/api/v1/operations/{id}"),
        format!("/api/v1/operations/{id}/trace"),
        format!("/api/v1/operations/{id}/steps/{step}"),
    ] {
        let reply = a.get(&uri, &login).await;
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
    }
    for action in ["approve", "deny"] {
        let uri = format!("/api/v1/operations/{id}/steps/{step}/{action}");
        let reply = a.post(&uri, &login, json!({})).await;
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
    }
    let incident = a
        .get(&format!("/api/v1/incidents/{}", a.incident.id()), &login)
        .await;
    assert_eq!(incident.body["operations"], json!([]));
}
