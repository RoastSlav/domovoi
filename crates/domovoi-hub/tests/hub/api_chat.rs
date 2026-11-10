//! The general chat: the limits on what it accepts, the overview the model sees, and the
//! refusal when triage is off.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::api_harness::{api, api_with_triage};

#[tokio::test]
async fn chat_rejects_oversized_or_malformed_requests_without_a_model_call() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let cases: [(&str, Value); 5] = [
        ("an empty question", json!({ "question": "   " })),
        (
            "a question over the limit",
            json!({ "question": "q".repeat(1_001) }),
        ),
        (
            "too many earlier turns",
            json!({
                "question": "Why?",
                "history": (0..21)
                    .map(|_| json!({ "role": "assistant", "text": "Earlier." }))
                    .collect::<Vec<Value>>(),
            }),
        ),
        (
            "an earlier turn over the limit",
            json!({
                "question": "Why?",
                "history": [{ "role": "user", "text": "x".repeat(4_001) }],
            }),
        ),
        (
            "an unknown role",
            json!({
                "question": "Why?",
                "history": [{ "role": "system", "text": "Earlier." }],
            }),
        ),
    ];
    for (what, body) in cases {
        let reply = a.post("/api/v1/chat", &login, body).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{what}: {}",
            reply.text
        );
    }
    assert!(
        a.llm.requests().is_empty(),
        "no rejected request reached the model"
    );
}

#[tokio::test]
async fn chat_about_an_unknown_host_is_404() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let reply = a
        .post(
            "/api/v1/chat",
            &login,
            json!({ "host": "ghost", "question": "Why is it slow?" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);
    assert!(
        a.llm.requests().is_empty(),
        "an unknown host never reaches the model"
    );
}

#[tokio::test]
async fn chat_answers_from_the_model_with_every_host_in_its_overview() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    a.llm.push_text("The disk is filling up.");

    let reply = a
        .post(
            "/api/v1/chat",
            &login,
            json!({ "question": "Why is the NAS slow?" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["answer"], "The disk is filling up.");

    let asked = a.llm.requests();
    assert_eq!(asked.len(), 1, "the model answers once");
    let seen = format!("{:?}", asked[0]);
    assert!(
        seen.contains("Why is the NAS slow?"),
        "the question reaches the model: {seen}"
    );
    assert!(seen.contains("nas"), "the overview names the hosts: {seen}");
    assert!(
        seen.contains("disk.usage"),
        "the overview lists the open incident: {seen}"
    );
}

#[tokio::test]
async fn chat_without_triage_is_503() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .post("/api/v1/chat", &login, json!({ "question": "Why?" }))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
}
