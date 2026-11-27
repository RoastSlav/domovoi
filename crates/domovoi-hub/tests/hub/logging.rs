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
        "INFO",
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

async fn login_as(a: &crate::api_harness::Api, user: &str, password: &str) -> StatusCode {
    a.send(
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(serde_json::json!({ "username": user, "password": password })),
    )
    .await
    .status
}

#[tokio::test]
async fn a_failed_login_names_the_user_and_the_reason() {
    let a = api().await;
    a.admin().await;
    let (logs, _guard) = LogCapture::start();
    assert_eq!(
        login_as(&a, USERNAME, "wrong wrong wrong").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_as(&a, "ghost", "whatever it is").await,
        StatusCode::UNAUTHORIZED
    );
    let lines = logs.lines_with("failed login");
    assert_eq!(lines.len(), 2, "{}", logs.text());
    assert!(lines[0].contains("username=\"admin\""), "{}", lines[0]);
    assert!(
        lines[0].contains("reason=\"wrong password\""),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("ip=10.0.0.7"), "{}", lines[0]);
    assert!(lines[1].contains("username=\"ghost\""), "{}", lines[1]);
    assert!(lines[1].contains("reason=\"unknown user\""), "{}", lines[1]);
    assert!(
        !logs.text().contains("wrong wrong wrong"),
        "no password is logged"
    );
}

#[tokio::test]
async fn a_lockout_is_logged_once_when_it_starts() {
    let a = api().await;
    a.admin().await;
    let (logs, _guard) = LogCapture::start();
    for _ in 0..8 {
        login_as(&a, USERNAME, "wrong wrong wrong").await;
    }
    let lines = logs.lines_with("locked out");
    assert_eq!(lines.len(), 1, "one line for the lockout: {}", logs.text());
    assert!(lines[0].contains("lockout_secs=60"), "{}", lines[0]);
    assert!(lines[0].contains("username=\"admin\""), "{}", lines[0]);
}

#[tokio::test]
async fn a_failed_password_confirmation_names_the_route_and_the_user() {
    let a = api().await;
    let login = a.admin().await;
    a.post(
        "/api/v1/policy/autonomy/pause",
        &login,
        serde_json::json!({ "reason": "maintenance" }),
    )
    .await;
    let (logs, _guard) = LogCapture::start();
    a.post(
        "/api/v1/policy/autonomy/resume",
        &login,
        serde_json::json!({ "password": "wrong" }),
    )
    .await;
    let lines = logs.lines_with("failed password confirmation");
    assert_eq!(lines.len(), 1, "{}", logs.text());
    for field in [
        "path=/api/v1/policy/autonomy/resume",
        "method=POST",
        "username=admin",
        "request_id=",
    ] {
        assert!(lines[0].contains(field), "lacks {field}: {}", lines[0]);
    }
}

#[tokio::test]
async fn an_enrollment_invite_names_who_created_it_and_the_host_id() {
    let a = api().await;
    let login = a.admin().await;
    let (logs, _guard) = LogCapture::start();
    let reply = a
        .send(
            Method::POST,
            "/api/v1/hosts/nas/enrollment",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let host_id = reply.body["host_id"].as_str().unwrap().to_owned();
    let token = reply.body["token"].as_str().unwrap().to_owned();
    let line = logs.lines_with("enrollment invite created").remove(0);
    for field in [
        "host=nas".to_owned(),
        format!("host_id={host_id}"),
        "actor=admin".to_owned(),
    ] {
        assert!(line.contains(&field), "lacks {field}: {line}");
    }
    assert!(!logs.text().contains(&token), "the token is never logged");
}

#[tokio::test]
async fn a_saved_setting_logs_keys_actor_revision_and_an_audit_pointer_without_values() {
    use crate::api_harness::PASSWORD;
    let a = api().await;
    let login = a.admin().await;
    let secret = "discord-log-secret-not-for-eyes";
    let (logs, _guard) = LogCapture::start();
    let reply = a
        .send(
            Method::PATCH,
            "/api/v1/settings",
            Some(&login),
            Some(serde_json::json!({
                "base_revision": 0,
                "password": PASSWORD,
                "changes": { "discord": {
                    "enabled": false,
                    "channel_id": 42,
                    "bot_token": { "set": secret }
                }, "incidents": { "confirm_runs": 4 } }
            })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let saved = logs.lines_with("settings saved").remove(0);
    for field in [
        "revision=1",
        "actor=admin",
        "incidents.confirm_runs",
        "discord.bot_token",
    ] {
        assert!(saved.contains(field), "lacks {field}: {saved}");
    }
    let pointer = logs.lines_with("kind=settings.changed").remove(0);
    assert!(
        pointer.contains("audit row recorded") && pointer.contains("seq="),
        "{pointer}"
    );
    assert!(
        !logs.text().contains(secret),
        "a secret never reaches the log"
    );
}

#[tokio::test]
async fn a_correct_login_on_a_full_disk_is_a_clear_503_and_not_a_failed_login() {
    use domovoi_core::ports::{AuditFilter, AuditLog, Page};

    use crate::api_harness::PASSWORD;

    let a = api().await;
    a.admin().await;
    // The one writer connection now refuses every write, as it does on a full disk.
    sqlx::query("PRAGMA query_only = 1")
        .execute(a.store.pool_for_tests())
        .await
        .unwrap();
    let (logs, _guard) = LogCapture::start();
    let reply = a
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(serde_json::json!({ "username": USERNAME, "password": PASSWORD })),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["error"], "store_unwritable");
    let message = reply.body["message"].as_str().unwrap();
    assert!(message.contains("disk is probably full"), "{message}");
    assert!(message.contains("Free space"), "{message}");

    let lines = logs.lines_with("the hub cannot write to its database");
    assert_eq!(
        lines.len(),
        1,
        "one line, not one per failure: {}",
        logs.text()
    );
    assert!(lines[0].contains("ERROR"), "{}", lines[0]);
    assert!(lines[0].contains("cause="), "{}", lines[0]);
    assert!(lines[0].contains("free disk space"), "{}", lines[0]);

    let failed = AuditFilter {
        kind: Some("auth.login_failed".to_owned()),
        ..AuditFilter::default()
    };
    let rows = a.store.list(&failed, &Page::first(10)).await.unwrap().items;
    assert!(
        rows.is_empty(),
        "a disk failure is not a failed login: {rows:?}"
    );
    assert!(
        logs.lines_with("failed login").is_empty(),
        "{}",
        logs.text()
    );
}
