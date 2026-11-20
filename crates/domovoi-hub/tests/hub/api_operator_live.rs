//! The live frames of an Operator answer on the event stream: no ids, no replay, only for
//! the followed conversation, never a secret, and never able to stall a turn.

use axum::http::{Method, StatusCode};
use domovoi_core::ports::{LiveEvent, LivePublisher, ToolPhase};
use domovoi_core::{ConversationId, TurnId};
use serde_json::{Value, json};

use crate::api_events::{Stream, data, id, name, open};
use crate::api_harness::{Api, Login, api_with_triage};

async fn conversation(a: &Api, login: &Login) -> String {
    let reply = a
        .post(
            "/api/v1/conversations",
            login,
            json!({ "scope": { "kind": "global" } }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    reply.body["id"].as_str().unwrap().to_owned()
}

async fn ask(a: &Api, login: &Login, conversation: &str, text: &str) -> Value {
    let reply = a
        .post(
            &format!("/api/v1/conversations/{conversation}/messages"),
            login,
            json!({ "turn": TurnId::new().to_string(), "text": text }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.text);
    reply.body
}

async fn follow(a: &Api, login: &Login, conversation: &str) -> Stream {
    let mut s = open(
        a,
        login,
        &format!("/api/v1/events?topics=conversations:{conversation}"),
        None,
    )
    .await;
    assert_eq!(s.status, StatusCode::OK);
    assert_eq!(name(&s.next().await), "ready");
    s
}

/// Every frame up to and including `live.end`.
async fn until_end(s: &mut Stream) -> Vec<String> {
    let mut frames = Vec::new();
    loop {
        let frame = s.next().await;
        let done = name(&frame) == "live.end";
        frames.push(frame);
        if done {
            return frames;
        }
    }
}

async fn settled(a: &Api, login: &Login, conversation: &str) {
    for _ in 0..200 {
        let reply = a
            .get(&format!("/api/v1/conversations/{conversation}"), login)
            .await;
        if reply.body["turn_in_progress"].is_null() {
            return;
        }
        let seen = reply.body["as_of_seq"].as_u64().unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            domovoi_core::ports::JournalFeed::wait_after(&*a.state.deps().journal, seen),
        )
        .await
        .expect("the answer settles");
    }
    panic!("the answer never settled");
}

#[tokio::test]
async fn a_conversation_stream_delivers_live_frames_without_ids() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    a.llm
        .push_streamed(&["The disk ", "is filling ", "up fast. "]);
    ask(&a, &login, &c, "Why is the NAS slow?").await;
    let frames = until_end(&mut s).await;
    let names: Vec<&str> = frames.iter().map(|f| name(f)).collect();
    assert!(names.contains(&"live.start"), "{names:?}");
    assert!(names.contains(&"live.text"), "{names:?}");
    assert_eq!(names.last(), Some(&"live.end"));
    for frame in &frames {
        match name(frame) {
            n if n.starts_with("live.") => {
                assert_eq!(id(frame), None, "a live frame has no id: {frame}");
                assert_eq!(data(frame)["conversation"], c);
            }
            "journal" => assert!(id(frame).is_some(), "journal frames keep their ids"),
            other => panic!("unexpected frame {other}"),
        }
    }
}

#[tokio::test]
async fn live_frames_reach_only_the_subscribed_conversation() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let followed = conversation(&a, &login).await;
    let other = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &followed).await;
    a.llm.push_text("Other answer.");
    ask(&a, &login, &other, "Why?").await;
    settled(&a, &login, &other).await;
    assert!(
        s.is_quiet().await,
        "nothing of the other conversation arrives"
    );
}

#[tokio::test]
async fn live_frames_need_the_conversation_topic() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = open(&a, &login, "/api/v1/events?topics=incidents", None).await;
    assert_eq!(name(&s.next().await), "ready");
    a.llm.push_text("Answer.");
    ask(&a, &login, &c, "Why?").await;
    settled(&a, &login, &c).await;
    assert!(
        s.is_quiet().await,
        "no live frame without the conversation topic"
    );
}

#[tokio::test]
async fn live_frames_are_never_replayed() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let head = domovoi_core::ports::JournalRead::head_seq(&*a.store)
        .await
        .unwrap();
    a.llm.push_text("Answer.");
    ask(&a, &login, &c, "Why?").await;
    settled(&a, &login, &c).await;
    let mut s = open(
        &a,
        &login,
        &format!("/api/v1/events?topics=conversations:{c}"),
        Some(&head.to_string()),
    )
    .await;
    assert_eq!(data(&s.next().await)["resumed"], true);
    for _ in 0..3 {
        let frame = s.next().await;
        assert_eq!(name(&frame), "journal", "{frame}");
    }
    assert!(s.is_quiet().await, "no live.* frame is replayed");
}

#[tokio::test]
async fn live_text_never_contains_a_secret() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    a.llm.push_streamed(&[
        "Here it is: -----BEGIN PRIVATE KEY-----\nMIIEvQSECRETBODY",
        "MORE\n-----END PRIVATE KEY-----\nthat was it. ",
    ]);
    ask(&a, &login, &c, "Show me the key").await;
    for frame in until_end(&mut s).await {
        assert!(!frame.contains("SECRETBODY"), "{frame}");
        assert!(!frame.contains("BEGIN PRIVATE"), "{frame}");
    }
}

#[tokio::test]
async fn a_lagging_live_subscriber_does_not_block_the_turn() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    let conversation_id: ConversationId = c.parse().unwrap();
    let turn = TurnId::new();
    for _ in 0..400 {
        a.state.deps().live.publish(
            conversation_id,
            LiveEvent::Tool {
                turn,
                tool: "journal".to_owned(),
                phase: ToolPhase::Started,
                failed: false,
            },
        );
    }
    a.llm.push_text("Answer.");
    ask(&a, &login, &c, "Why?").await;
    settled(&a, &login, &c).await;
    // The stream was never read: live frames were dropped, but the committed message arrives.
    let mut saw_complete = false;
    for _ in 0..1_000 {
        let frame = s.next().await;
        if name(&frame) == "journal" && data(&frame)["payload"]["status"] == "complete" {
            saw_complete = true;
            break;
        }
    }
    assert!(saw_complete, "the committed answer reaches a slow client");
}

#[tokio::test]
async fn live_end_follows_the_committed_message_frame() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    a.llm.push_text("Answer. ");
    ask(&a, &login, &c, "Why?").await;
    let frames = until_end(&mut s).await;
    let complete = frames
        .iter()
        .position(|f| name(f) == "journal" && data(f)["payload"]["status"] == "complete")
        .expect("the committed message frame arrives before live.end");
    assert!(complete < frames.len() - 1);
}

#[tokio::test]
async fn closing_the_stream_does_not_stop_the_turn() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    a.llm.hold();
    a.llm.push_text("Still answered.");
    ask(&a, &login, &c, "Why?").await;
    a.llm.wait_for_request().await;
    loop {
        if name(&s.next().await) == "live.start" {
            break;
        }
    }
    drop(s);
    a.llm.release(1);
    settled(&a, &login, &c).await;
    let detail = a.get(&format!("/api/v1/conversations/{c}"), &login).await;
    assert_eq!(
        detail.body["messages"][1]["status"], "complete",
        "{}",
        detail.text
    );
    let _ = Method::GET;
}

#[tokio::test]
async fn keepalive_still_works_with_live_frames() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let c = conversation(&a, &login).await;
    let mut s = follow(&a, &login, &c).await;
    a.state.deps().live.publish(
        c.parse().unwrap(),
        LiveEvent::Tool {
            turn: TurnId::new(),
            tool: "journal".to_owned(),
            phase: ToolPhase::Started,
            failed: false,
        },
    );
    assert_eq!(name(&s.next().await), "live.tool");
    a.advance(jiff::SignedDuration::from_secs(14));
    assert!(s.is_quiet().await, "a live frame reset the keepalive timer");
    a.advance(jiff::SignedDuration::from_secs(1));
    assert_eq!(s.next().await.trim(), ": keepalive");
}
