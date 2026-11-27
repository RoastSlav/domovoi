//! What the hub writes to its log while serving requests, so a failure can be traced back
//! to the request that caused it.

use axum::http::{Method, StatusCode};
use domovoi_testkit::log_capture::LogCapture;

use crate::api_harness::{USERNAME, api};

#[tokio::test]
async fn every_response_carries_a_request_id_that_the_log_line_repeats() {
    let a = api().await;
    let (logs, _guard) = LogCapture::start();
    let reply = a.send(Method::GET, "/api/v1/hosts", None, None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let id = reply.headers["x-request-id"].to_str().unwrap().to_owned();
    assert!(id.len() >= 16, "an id long enough to be unique: {id}");
    let lines = logs.lines_with(&format!("request_id={id}"));
    assert_eq!(lines.len(), 1, "{}", logs.text());
    for field in [
        "method=GET",
        "path=/api/v1/hosts",
        "status=401",
        "user=-",
        "duration_ms=",
        "WARN",
    ] {
        assert!(lines[0].contains(field), "line lacks {field}: {}", lines[0]);
    }
    let other = a.send(Method::GET, "/api/v1/hosts", None, None).await;
    assert_ne!(other.headers["x-request-id"], reply.headers["x-request-id"]);
}

#[tokio::test]
async fn the_request_line_names_the_signed_in_user() {
    let a = api().await;
    let login = a.admin().await;
    let (logs, _guard) = LogCapture::start();
    let reply = a.get("/api/v1/hosts", &login).await;
    let id = reply.headers["x-request-id"].to_str().unwrap().to_owned();
    let lines = logs.lines_with(&format!("request_id={id}"));
    assert_eq!(lines.len(), 1, "{}", logs.text());
    assert!(
        lines[0].contains(&format!("user={USERNAME}")),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("status=200"), "{}", lines[0]);
}
