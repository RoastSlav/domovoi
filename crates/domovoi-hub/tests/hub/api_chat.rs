//! The general chat: the limits on what it accepts, the overview the model sees, the
//! refusal when the hour's model runs are used up, and the refusal when triage is off.

use axum::http::{StatusCode, header};
use domovoi_core::ports::Presence;
use serde_json::{Value, json};

use crate::api_harness::{api, api_with_triage};

#[tokio::test]
async fn chat_rejects_oversized_or_malformed_requests_without_a_model_call() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let cases: [(&str, Value); 7] = [
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
            "an empty earlier turn",
            json!({
                "question": "Why?",
                "history": [{ "role": "user", "text": "" }],
            }),
        ),
        (
            "a whitespace-only earlier turn",
            json!({
                "question": "Why?",
                "history": [{ "role": "user", "text": "   " }],
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
async fn chat_about_an_offline_host_offers_the_model_no_tools() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    a.link
        .set_presence(a.nas.id, Presence::Disconnected { last_seen: None });
    a.llm
        .push_text("The NAS is offline, so I can only use what the hub knows.");

    let reply = a
        .post(
            "/api/v1/chat",
            &login,
            json!({ "host": "nas", "question": "Why is it slow?" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);

    let asked = a.llm.requests();
    assert_eq!(asked.len(), 1, "the model answers once");
    assert!(
        asked[0].tools.is_empty(),
        "an offline host offers the model no tools: {:?}",
        asked[0]
    );
    let seen = format!("{:?}", asked[0]);
    assert!(
        seen.contains("nas"),
        "the overview still names the offline host: {seen}"
    );
}

#[tokio::test]
async fn chat_answers_429_when_the_hourly_limit_is_reached() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    // The harness triage has the default limits: 6 runs an hour, 2 of them kept for
    // triage, so 4 questions share the hour and the next one has to wait.
    for _ in 0..4 {
        a.llm.push_text("Answered.");
        let reply = a
            .post(
                "/api/v1/chat",
                &login,
                json!({ "question": "Why is the NAS slow?" }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    }
    let reply = a
        .post(
            "/api/v1/chat",
            &login,
            json!({ "question": "One more question?" }),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        reply.text
    );
    assert_eq!(
        a.llm.requests().len(),
        4,
        "the refused question never reached the model"
    );
    let retry_after = reply.headers[header::RETRY_AFTER].to_str().unwrap();
    assert!(
        retry_after.parse::<u64>().is_ok_and(|seconds| seconds > 0),
        "Retry-After says when to come back: {retry_after} in {}",
        reply.text
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
