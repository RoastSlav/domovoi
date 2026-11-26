//! `GET /api/v1/events`: replay, resets, topics, keepalive and the stream cap.

use std::time::Duration;

use axum::body::BodyDataStream;
use axum::http::{Request, StatusCode, header};
use domovoi_core::ports::{
    AggregateRef, JournalDraft, JournalKind, JournalRead, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, IncidentId, NoteId, OperationId};
use futures::StreamExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api_harness::{Api, Login, api};

const LAG_LIMIT_ROWS: usize = 5_000;

fn draft(aggregate: AggregateRef, kind: JournalKind, payload: Value) -> JournalDraft {
    JournalDraft {
        aggregate,
        kind,
        payload_v: 1,
        payload,
        actor: Actor::System("test".to_owned()),
        occurred_at: "2026-11-18T10:00:00Z".parse().unwrap(),
    }
}

fn incident_row() -> JournalDraft {
    draft(
        AggregateRef::incident(IncidentId::new()),
        JournalKind::IncidentTimeline,
        json!({"kind": "status.changed", "summary": "s", "detail": null}),
    )
}

fn operation_row(op: OperationId, kind: JournalKind) -> JournalDraft {
    draft(AggregateRef::operation(op), kind, json!({"step": "x"}))
}

async fn commit(a: &Api, rows: Vec<JournalDraft>) -> Vec<u64> {
    let mut tx = Transaction::new();
    for row in rows {
        tx = tx.journal(row);
    }
    a.store.commit(tx).await.unwrap().journal_seqs
}

pub(crate) struct Stream {
    pub(crate) status: StatusCode,
    body: BodyDataStream,
}

pub(crate) async fn open(a: &Api, login: &Login, uri: &str, last_event_id: Option<&str>) -> Stream {
    let mut req = Request::get(uri).header(header::COOKIE, &login.cookie);
    if let Some(id) = last_event_id {
        req = req.header("last-event-id", id);
    }
    let response = a
        .app
        .clone()
        .oneshot(req.body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    Stream {
        status: response.status(),
        body: response.into_body().into_data_stream(),
    }
}

impl Stream {
    pub(crate) async fn next(&mut self) -> String {
        let chunk = tokio::time::timeout(Duration::from_secs(10), self.body.next())
            .await
            .expect("a frame arrives")
            .expect("the stream is open")
            .unwrap();
        String::from_utf8_lossy(&chunk).into_owned()
    }

    /// Nothing is waiting on the stream right now.
    pub(crate) async fn is_quiet(&mut self) -> bool {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        futures::FutureExt::now_or_never(self.body.next()).is_none()
    }
}

pub(crate) fn name(frame: &str) -> &str {
    frame
        .lines()
        .find_map(|l| l.strip_prefix("event: "))
        .unwrap_or("")
}

pub(crate) fn id(frame: &str) -> Option<u64> {
    frame
        .lines()
        .find_map(|l| l.strip_prefix("id: "))
        .map(|v| v.parse().unwrap())
}

pub(crate) fn data(frame: &str) -> Value {
    let text = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap_or("null");
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn sse_replays_missed_events() {
    let a = api().await;
    let login = a.admin().await;
    let seqs = commit(&a, vec![incident_row(), incident_row(), incident_row()]).await;
    let mut s = open(&a, &login, "/api/v1/events", Some(&seqs[0].to_string())).await;
    assert_eq!(s.status, StatusCode::OK);
    let ready = s.next().await;
    assert_eq!(name(&ready), "ready");
    assert_eq!(id(&ready), None, "a resumed ready carries no id");
    assert_eq!(data(&ready)["resumed"], true);
    for seq in [seqs[1], seqs[2]] {
        let frame = s.next().await;
        assert_eq!(name(&frame), "journal");
        assert_eq!(id(&frame), Some(seq));
        assert_eq!(data(&frame)["seq"], seq);
    }
    assert!(s.is_quiet().await);
}

#[tokio::test]
async fn after_parameter_resumes_like_last_event_id_and_the_header_wins() {
    let a = api().await;
    let login = a.admin().await;
    let seqs = commit(&a, vec![incident_row(), incident_row(), incident_row()]).await;
    let mut s = open(
        &a,
        &login,
        &format!("/api/v1/events?after={}", seqs[0]),
        None,
    )
    .await;
    assert_eq!(name(&s.next().await), "ready");
    assert_eq!(id(&s.next().await), Some(seqs[1]));

    let uri = format!("/api/v1/events?after={}", seqs[0]);
    let mut s = open(&a, &login, &uri, Some(&seqs[1].to_string())).await;
    assert_eq!(name(&s.next().await), "ready");
    assert_eq!(id(&s.next().await), Some(seqs[2]));
}

#[tokio::test]
async fn ready_without_a_cursor_carries_the_head_and_starts_the_live_tail() {
    let a = api().await;
    let login = a.admin().await;
    commit(&a, vec![incident_row(), incident_row()]).await;
    let head = a.store.head_seq().await.unwrap();
    let mut s = open(&a, &login, "/api/v1/events", None).await;
    let ready = s.next().await;
    assert_eq!(name(&ready), "ready");
    assert_eq!(id(&ready), Some(head));
    assert_eq!(data(&ready)["head_seq"], head);
    assert_eq!(data(&ready)["resumed"], false);
    assert_eq!(
        data(&ready)["topics"],
        json!(["approvals", "incidents", "operations"])
    );
    assert!(s.is_quiet().await, "earlier rows are not sent");
    let next = commit(&a, vec![incident_row()]).await[0];
    assert_eq!(id(&s.next().await), Some(next));
}

#[tokio::test]
async fn sse_resets_beyond_retention() {
    let a = api().await;
    let login = a.admin().await;
    let before = a.store.head_seq().await.unwrap();
    commit(&a, (0..5).map(|_| incident_row()).collect()).await;
    let head = before + 5;
    a.store.prune_journal_through(before + 3).await.unwrap();
    let mut s = open(
        &a,
        &login,
        "/api/v1/events",
        Some(&(before + 1).to_string()),
    )
    .await;
    let reset = s.next().await;
    assert_eq!(name(&reset), "reset");
    assert_eq!(id(&reset), Some(head));
    assert_eq!(data(&reset), json!({"reason": "pruned", "head_seq": head}));
    let next = commit(&a, vec![incident_row()]).await[0];
    assert_eq!(id(&s.next().await), Some(next));

    let mut s = open(
        &a,
        &login,
        "/api/v1/events",
        Some(&(before + 3).to_string()),
    )
    .await;
    assert_eq!(name(&s.next().await), "ready");
    assert_eq!(id(&s.next().await), Some(before + 4));
    assert_eq!(id(&s.next().await), Some(before + 5));
}

#[tokio::test]
async fn sse_resets_when_the_cursor_is_ahead_of_the_head() {
    let a = api().await;
    let login = a.admin().await;
    let head = a.store.head_seq().await.unwrap();
    let mut s = open(
        &a,
        &login,
        "/api/v1/events",
        Some(&(head + 100).to_string()),
    )
    .await;
    let reset = s.next().await;
    assert_eq!(name(&reset), "reset");
    assert_eq!(data(&reset)["reason"], "ahead");
}

#[tokio::test]
async fn lagged_subscriber_loses_nothing() {
    let a = api().await;
    let login = a.admin().await;
    let mut s = open(&a, &login, "/api/v1/events", None).await;
    assert_eq!(name(&s.next().await), "ready");
    let mut expected = Vec::new();
    for _ in 0..3 {
        expected.extend(commit(&a, (0..100).map(|_| incident_row()).collect()).await);
    }
    for seq in expected {
        let frame = s.next().await;
        assert_eq!(name(&frame), "journal", "{frame}");
        assert_eq!(id(&frame), Some(seq));
    }
    assert!(s.is_quiet().await);
}

#[tokio::test]
async fn sse_resets_a_client_that_falls_too_far_behind() {
    let a = api().await;
    let login = a.admin().await;
    let mut s = open(&a, &login, "/api/v1/events", None).await;
    assert_eq!(name(&s.next().await), "ready");
    commit(&a, (0..=LAG_LIMIT_ROWS).map(|_| incident_row()).collect()).await;
    let head = a.store.head_seq().await.unwrap();
    let reset = s.next().await;
    assert_eq!(name(&reset), "reset");
    assert_eq!(data(&reset), json!({"reason": "lagged", "head_seq": head}));
    let next = commit(&a, vec![incident_row()]).await[0];
    assert_eq!(id(&s.next().await), Some(next));
}

#[tokio::test]
async fn sse_filters_by_topic() {
    let a = api().await;
    let login = a.admin().await;
    let (op_a, op_b) = (OperationId::new(), OperationId::new());
    let seqs = commit(
        &a,
        vec![
            incident_row(),
            operation_row(op_a, JournalKind::StepFinished),
            operation_row(op_b, JournalKind::StepFinished),
            operation_row(op_a, JournalKind::ExecutionTransition),
            draft(
                AggregateRef::note(NoteId::new()),
                JournalKind::IncidentTimeline,
                json!({}),
            ),
        ],
    )
    .await;
    let first = seqs[0] - 1;
    let cases = [
        ("incidents".to_owned(), vec![seqs[0]]),
        ("approvals".to_owned(), vec![seqs[3]]),
        ("operations".to_owned(), vec![seqs[1], seqs[2], seqs[3]]),
        (
            format!("operation:{}", op_a.as_uuid()),
            vec![seqs[1], seqs[3]],
        ),
    ];
    for (topic, want) in cases {
        let uri = format!("/api/v1/events?topics={topic}");
        let mut s = open(&a, &login, &uri, Some(&first.to_string())).await;
        assert_eq!(name(&s.next().await), "ready");
        for seq in &want {
            assert_eq!(id(&s.next().await), Some(*seq), "{topic}");
        }
        assert!(s.is_quiet().await, "{topic}");
    }
}

#[tokio::test]
async fn keepalive_is_sent_after_fifteen_seconds_of_clock_time() {
    let a = api().await;
    let login = a.admin().await;
    let mut s = open(&a, &login, "/api/v1/events", None).await;
    assert_eq!(name(&s.next().await), "ready");
    assert!(s.is_quiet().await);
    a.advance(jiff::SignedDuration::from_secs(15));
    assert_eq!(s.next().await.trim(), ": keepalive");

    commit(&a, vec![incident_row()]).await;
    assert_eq!(name(&s.next().await), "journal");
    a.advance(jiff::SignedDuration::from_secs(14));
    assert!(s.is_quiet().await);
    a.advance(jiff::SignedDuration::from_secs(1));
    assert_eq!(s.next().await.trim(), ": keepalive");
}

#[tokio::test]
async fn invalid_topic_is_a_bad_request() {
    let a = api().await;
    let login = a.admin().await;
    let many: Vec<String> = (0..17)
        .map(|n| format!("operation:0190a0a0-0000-7000-8000-{n:012}"))
        .collect();
    for uri in [
        "/api/v1/events?topics=incidents,bogus".to_owned(),
        "/api/v1/events?topics=operation:not-a-uuid".to_owned(),
        format!("/api/v1/events?topics={}", many.join(",")),
        "/api/v1/events?after=abc".to_owned(),
    ] {
        let s = open(&a, &login, &uri, None).await;
        assert_eq!(s.status, StatusCode::BAD_REQUEST, "{uri}");
    }
    let s = open(&a, &login, "/api/v1/events", Some("x")).await;
    assert_eq!(s.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn too_many_streams_are_refused_and_the_slot_frees_on_disconnect() {
    let a = api().await;
    let login = a.admin().await;
    let mut open_streams = Vec::new();
    for _ in 0..32 {
        let s = open(&a, &login, "/api/v1/events", None).await;
        assert_eq!(s.status, StatusCode::OK);
        open_streams.push(s);
    }
    let refused = open(&a, &login, "/api/v1/events", None).await;
    assert_eq!(refused.status, StatusCode::SERVICE_UNAVAILABLE);
    drop(open_streams.pop());
    let again = open(&a, &login, "/api/v1/events", None).await;
    assert_eq!(again.status, StatusCode::OK);
}

#[tokio::test]
async fn snapshots_carry_as_of_seq() {
    let a = api().await;
    let login = a.admin().await;
    let id = a.incident.id();
    let uris = [
        "/api/v1/incidents".to_owned(),
        format!("/api/v1/incidents/{id}"),
        "/api/v1/overview".to_owned(),
    ];
    for uri in uris {
        let head = a.store.head_seq().await.unwrap();
        let reply = a.get(&uri, &login).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        assert_eq!(reply.body["as_of_seq"], head, "{uri}");
        commit(&a, vec![incident_row()]).await;
        let later = a.get(&uri, &login).await;
        assert!(later.body["as_of_seq"].as_u64().unwrap() > head, "{uri}");
    }
}
