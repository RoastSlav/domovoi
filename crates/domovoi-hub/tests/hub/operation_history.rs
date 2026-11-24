//! `GET /api/v1/operations/{id}/history`: the projection of an operation's journal rows that
//! the web UI replays the graph from.

use axum::http::{Method, StatusCode};
use domovoi_core::ports::{AggregateRef, JournalDraft, JournalKind, Transaction, UnitOfWork};
use domovoi_core::{Actor, OperationId, StepId, StepIntent, StepKind, StepOutcome};
use domovoi_engine::operations::{NewStep, StepFinish};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api, api_with_operations};

const MARKER: &str = "SECRET-MARKER-9";

fn uri(op: impl std::fmt::Display) -> String {
    format!("/api/v1/operations/{op}/history")
}

async fn history(a: &Api, login: &Login, op: OperationId) -> Value {
    let reply = a.get(&uri(op), login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

fn events(body: &Value) -> &Vec<Value> {
    body["events"].as_array().unwrap()
}

fn kinds(body: &Value) -> Vec<&str> {
    events(body)
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect()
}

async fn commit_rows(a: &Api, op: OperationId, count: usize, kind: JournalKind) {
    let mut tx = Transaction::new();
    for n in 0..count {
        tx = tx.journal(JournalDraft {
            aggregate: AggregateRef::operation(op),
            kind,
            payload_v: 1,
            payload: json!({ "step": format!("extra-{n}") }),
            actor: Actor::System("tests".to_owned()),
            occurred_at: "2026-11-18T10:00:00Z".parse().unwrap(),
        });
    }
    a.store.commit(tx).await.unwrap();
}

/// A note step whose intent, summary and reason all carry [`MARKER`].
async fn marked_step(a: &Api, op: OperationId) -> StepId {
    let step = StepId::new();
    a.ops()
        .propose(
            op,
            vec![NewStep {
                id: step,
                kind: StepKind::ReasoningTurn,
                phase: "investigate".to_owned(),
                parent: None,
                intent: StepIntent::ReasoningTurn {
                    context_refs: vec![MARKER.to_owned()],
                },
            }],
            Vec::new(),
        )
        .await
        .unwrap();
    a.ops()
        .step_finished(
            op,
            step,
            StepFinish {
                outcome: StepOutcome::Succeeded,
                summary: Some(MARKER.to_owned()),
                reason: Some(MARKER.to_owned()),
                output: Some(MARKER.to_owned()),
                usage: None,
            },
        )
        .await
        .unwrap();
    step
}

#[tokio::test]
async fn history_lists_the_operation_rows_in_order_with_projected_fields() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let note = marked_step(&a, op).await;
    let exec = a.propose_restart(op).await;
    let detail = a.get(&format!("/api/v1/operations/{op}"), &login).await;
    let hash = detail.body["executions"][0]["plan_hash"].as_str().unwrap();
    let approved = a
        .post(
            &format!("/api/v1/operations/{op}/steps/{exec}/approve"),
            &login,
            json!({ "plan_hash": hash }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);

    let body = history(&a, &login, op).await;
    assert_eq!(body["complete"], true);
    assert!(body["incomplete_reason"].is_null());
    assert_eq!(body["as_of_seq"], a.head_seq().await);
    let seqs: Vec<u64> = events(&body)
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "ascending: {seqs:?}");
    assert_eq!(kinds(&body)[0], "operation.created");
    let created_for = |step: StepId| {
        events(&body)
            .iter()
            .filter(|e| e["kind"] == "operation.step_created" && e["step"] == step.to_string())
            .count()
    };
    assert_eq!(created_for(note), 1);
    assert_eq!(created_for(exec), 1);
    let states: Vec<&str> = events(&body)
        .iter()
        .filter(|e| e["kind"] == "operation.execution")
        .map(|e| e["state"].as_str().unwrap())
        .collect();
    assert!(states.contains(&"awaiting_approval"), "{states:?}");
    assert!(states.contains(&"authorized"), "{states:?}");
    assert!(
        events(&body)
            .iter()
            .filter(|e| e["kind"] == "operation.execution")
            .all(|e| e["step"] == exec.to_string())
    );
    let statuses: Vec<&Value> = events(&body)
        .iter()
        .filter(|e| e["kind"] == "operation.status")
        .collect();
    assert!(!statuses.is_empty());
    assert!(
        statuses
            .iter()
            .all(|e| e["state"].is_string() && e["step"].is_null())
    );
    assert!(
        events(&body)[0]["at"].as_str().unwrap().ends_with('Z'),
        "RFC 3339 UTC"
    );
}

#[tokio::test]
async fn history_never_returns_intent_summary_reason_or_payload_text() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    marked_step(&a, op).await;

    let reply = a.get(&uri(op), &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert!(!reply.text.contains(MARKER));
    assert!(!reply.text.contains("find out why"), "the goal stays home");
    for event in events(&reply.body) {
        let mut keys: Vec<&String> = event.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["at", "kind", "seq", "state", "step"]);
    }
    let mut top: Vec<&String> = reply.body.as_object().unwrap().keys().collect();
    top.sort();
    assert_eq!(
        top,
        ["as_of_seq", "complete", "events", "incomplete_reason"]
    );
}

#[tokio::test]
async fn history_reads_the_journal_head_before_the_rows() {
    // The race itself (a commit between the head read and the row read) is covered by the unit
    // test `build_history_drops_rows_after_the_snapshot_and_reports_gaps`; the store cannot be
    // hooked between the two calls from here. This checks the contract on the wire.
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    a.propose_restart(op).await;
    let head = a.head_seq().await;

    let body = history(&a, &login, op).await;
    assert_eq!(body["as_of_seq"], head);
    assert!(
        events(&body)
            .iter()
            .all(|e| e["seq"].as_u64().unwrap() <= head)
    );

    a.propose_run(op, "prune_images", &[]).await;
    let moved = a.head_seq().await;
    assert!(moved > head);
    let after = history(&a, &login, op).await;
    assert_eq!(after["as_of_seq"], moved);
    assert!(events(&after).len() > events(&body).len());
}

#[tokio::test]
async fn history_ignores_rows_of_other_aggregates() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("first", true).await;
    let other = a.create_operation("second", false).await;
    let other_step = a.propose_restart(other).await;
    commit_rows(&a, OperationId::new(), 2, JournalKind::StepCreated).await;

    let body = history(&a, &login, op).await;
    assert_eq!(
        kinds(&body),
        ["operation.created", "operation.status"],
        "{body}"
    );
    let theirs = history(&a, &login, other).await;
    assert!(
        events(&theirs)
            .iter()
            .any(|e| e["step"] == other_step.to_string())
    );
    assert!(
        events(&body)
            .iter()
            .all(|e| e["step"] != other_step.to_string())
    );
}

#[tokio::test]
async fn history_marks_a_pruned_head() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    a.propose_restart(op).await;
    let whole = history(&a, &login, op).await;
    let created = whole["events"][0]["seq"].as_u64().unwrap();
    assert_eq!(whole["events"][0]["kind"], "operation.created");

    a.store.prune_journal_through(created).await.unwrap();
    let body = history(&a, &login, op).await;
    assert_eq!(body["complete"], false);
    assert_eq!(body["incomplete_reason"], "pruned");
    assert!(!events(&body).is_empty());
    assert_ne!(events(&body)[0]["kind"], "operation.created");
}

#[tokio::test]
async fn history_with_every_row_pruned_is_pruned_and_empty() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    a.propose_restart(op).await;
    a.store
        .prune_journal_through(a.head_seq().await)
        .await
        .unwrap();

    let body = history(&a, &login, op).await;
    assert_eq!(body["events"], json!([]));
    assert_eq!(body["complete"], false);
    assert_eq!(body["incomplete_reason"], "pruned");
}

#[tokio::test]
async fn history_marks_a_capped_list() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    commit_rows(&a, op, 505, JournalKind::StepCreated).await;

    let body = history(&a, &login, op).await;
    assert_eq!(events(&body).len(), 500);
    assert_eq!(body["complete"], false);
    assert_eq!(body["incomplete_reason"], "capped");
    assert_eq!(events(&body)[0]["kind"], "operation.created");

    let first = events(&body)[0]["seq"].as_u64().unwrap();
    a.store.prune_journal_through(first).await.unwrap();
    let both = history(&a, &login, op).await;
    assert_eq!(events(&both).len(), 500);
    assert_eq!(both["incomplete_reason"], "both");
}

#[tokio::test]
async fn history_of_an_unknown_or_malformed_id_is_404() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    for id in [OperationId::new().to_string(), "not-an-id".to_owned()] {
        let reply = a.get(&uri(&id), &login).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{id}: {}", reply.text);
        assert_eq!(reply.body["error"], "not_found");
    }
}

#[tokio::test]
async fn history_answers_503_when_orchestration_is_off() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get(&uri(OperationId::new()), &login).await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
    let trace = a
        .get(
            &format!("/api/v1/operations/{}/trace", OperationId::new()),
            &login,
        )
        .await;
    assert_eq!(
        reply.body, trace.body,
        "the same text as the trace endpoint"
    );
}

#[tokio::test]
async fn history_requires_a_session() {
    let a = api_with_operations().await;
    a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let reply = a.send(Method::GET, &uri(op), None, None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_journal_kinds_pass_through_with_null_fields() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("find out why", false).await;
    let mut tx = Transaction::new();
    tx = tx.journal(JournalDraft {
        aggregate: AggregateRef::operation(op),
        kind: JournalKind::OperationSummary,
        payload_v: 7,
        payload: json!("not an object"),
        actor: Actor::System("tests".to_owned()),
        occurred_at: "2026-11-18T10:00:00Z".parse().unwrap(),
    });
    a.store.commit(tx).await.unwrap();

    let body = history(&a, &login, op).await;
    // The injected event carries a fixed past timestamp, so events recorded with the real clock
    // sort after it; find it by kind instead of assuming it is last.
    let summary = events(&body)
        .iter()
        .find(|e| e["kind"] == "operation.summary")
        .expect("the injected operation.summary event is in the history");
    assert!(summary["step"].is_null());
    assert!(summary["state"].is_null());
}
