use axum::http::StatusCode;
use serde_json::Value;

use crate::api_harness::{api, api_without_logs, canned_logs};

#[tokio::test]
async fn logs_endpoint_answers_with_the_captured_lines_oldest_first() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/system/logs", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let served: Vec<Value> = reply.body.as_array().unwrap().to_vec();
    let expected = canned_logs();
    assert_eq!(served.len(), expected.len());
    assert_eq!(served[0]["target"], expected[0].target);
    assert_eq!(served[0]["message"], expected[0].message);
    assert_eq!(served[1]["level"], expected[1].level);
    let last = expected[expected.len() - 1].clone();
    assert_eq!(served[2]["target"], last.target);
    assert_eq!(served[2]["level"], last.level);
    let message = served[2]["message"].as_str().unwrap();
    assert!(
        !message.contains(last.message.as_str()),
        "the credential in the last line was removed",
    );
    assert!(message.contains("[REDACTED]"), "{}", message);
}

#[tokio::test]
async fn logs_endpoint_answers_with_the_newest_lines_it_was_asked_for() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/system/logs?lines=2", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let served: Vec<Value> = reply.body.as_array().unwrap().to_vec();
    assert_eq!(served.len(), 2);
    let expected = canned_logs();
    let newest = expected[expected.len() - 1].clone();
    let second_newest = expected[expected.len() - 2].clone();
    assert_eq!(served[0]["target"], second_newest.target);
    assert_eq!(served[1]["target"], newest.target);
}

#[tokio::test]
async fn logs_endpoint_says_when_nothing_was_captured() {
    let a = api_without_logs().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/system/logs", &login).await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["error"], "unavailable");
    assert_eq!(reply.body["message"], "logs are not captured");
}
