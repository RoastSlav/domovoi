//! How far a known condition for a security check may reach, and the password it asks for.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, PASSWORD, api};

fn body(check: &str, host: Value, target: Value) -> Value {
    json!({
        "host": host,
        "check": check,
        "target_pattern": target,
        "predicates": [],
        "note": "a known condition",
        "expires_in_minutes": null,
    })
}

async fn post(a: &Api, login: &Login, body: Value) -> crate::api_harness::Reply {
    a.post("/api/v1/ack-rules", login, body).await
}

fn with(mut body: Value, field: &str, value: Value) -> Value {
    body[field] = value;
    body
}

async fn stored(a: &Api, login: &Login) -> usize {
    a.get("/api/v1/ack-rules", login)
        .await
        .body
        .as_array()
        .map_or(0, Vec::len)
}

#[tokio::test]
async fn a_security_check_rule_for_every_host_and_every_target_without_expiry_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    for check in [
        "security.sudo_failure",
        "security.ssh_login",
        "security.new_port",
        "kernel.errors",
    ] {
        let wide = with(
            body(check, Value::Null, Value::Null),
            "password",
            json!(PASSWORD),
        );
        let reply = post(&a, &login, wide).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{check}: {}",
            reply.text
        );
        let message = reply.body["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("every host and every target")
                && message.contains("Name a host and a target pattern")
                && message.contains("30 days"),
            "{check}: the sentence says what to add: {message}"
        );
    }
    // A bare `*` is every target, and a missing host is every host.
    for (host, target, reach) in [
        (json!("nas"), json!("*"), "every target"),
        (Value::Null, json!("22/tcp"), "every host"),
    ] {
        let reply = post(
            &a,
            &login,
            with(
                body("security.ssh_login", host, target),
                "password",
                json!(PASSWORD),
            ),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
        assert!(reply.text.contains(reach), "{reach}: {}", reply.text);
    }
    assert_eq!(stored(&a, &login).await, 0, "nothing refused was stored");
    assert!(a.audit_events("ack_rule.created").await.is_empty());
}

#[tokio::test]
async fn a_wide_rule_for_a_security_check_may_last_at_most_thirty_days() {
    let a = api().await;
    let login = a.admin().await;
    let wide = |minutes: u32| {
        with(
            with(
                body("security.sudo_failure", Value::Null, Value::Null),
                "expires_in_minutes",
                json!(minutes),
            ),
            "password",
            json!(PASSWORD),
        )
    };
    let too_long = post(&a, &login, wide(43_201)).await;
    assert_eq!(
        too_long.status,
        StatusCode::BAD_REQUEST,
        "{}",
        too_long.text
    );
    assert!(
        too_long.text.contains("at most 30 days"),
        "{}",
        too_long.text
    );
    let ok = post(&a, &login, wide(43_200)).await;
    assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.text);
    assert_eq!(ok.body["covers_every_host"], true);
    assert_eq!(ok.body["covers_every_target"], true);
}

#[tokio::test]
async fn a_security_check_rule_asks_for_the_password() {
    let a = api().await;
    let login = a.admin().await;
    let specific = body("security.ssh_login", json!("nas"), json!("alice"));
    let missing = post(&a, &login, specific.clone()).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    assert!(
        missing.text.contains("enter your password"),
        "{}",
        missing.text
    );
    let wrong = post(
        &a,
        &login,
        with(specific.clone(), "password", json!("nope")),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN, "{}", wrong.text);
    assert_eq!(stored(&a, &login).await, 0);
    let ok = post(&a, &login, with(specific, "password", json!(PASSWORD))).await;
    assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.text);
    assert_eq!(ok.body["covers_every_host"], false);
    assert_eq!(ok.body["covers_every_target"], false);
}

#[tokio::test]
async fn other_checks_need_no_password_and_may_cover_everything_but_not_with_double_star() {
    let a = api().await;
    let login = a.admin().await;
    let wide = body("disk.free", Value::Null, Value::Null);
    let ok = post(&a, &login, wide).await;
    assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.text);
    for check in ["disk.free", "security.ssh_login"] {
        let double = with(
            with(
                body(check, json!("nas"), json!("**")),
                "password",
                json!(PASSWORD),
            ),
            "expires_in_minutes",
            json!(60),
        );
        let reply = post(&a, &login, double).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{check}: {}",
            reply.text
        );
        assert!(
            reply.text.contains("`**` is not a target pattern"),
            "{}",
            reply.text
        );
    }
}

#[tokio::test]
async fn editing_what_a_security_rule_covers_asks_for_the_password_and_keeps_the_scope() {
    let a = api().await;
    let login = a.admin().await;
    let created = post(
        &a,
        &login,
        with(
            body("security.ssh_login", json!("nas"), json!("alice")),
            "password",
            json!(PASSWORD),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let uri = format!("/api/v1/ack-rules/{}", created.body["id"].as_str().unwrap());
    let edit = |change: Value| a.send(Method::PATCH, &uri, Some(&login), Some(change));

    // A note alone widens nothing: no password.
    let note = edit(json!({ "note": "alice from the office" })).await;
    assert_eq!(note.status, StatusCode::OK, "{}", note.text);
    // Widening to every host is refused without an expiry, whatever the password.
    let wide = edit(json!({ "host": null, "password": PASSWORD })).await;
    assert_eq!(wide.status, StatusCode::BAD_REQUEST, "{}", wide.text);
    assert!(wide.text.contains("every host"), "{}", wide.text);
    // A change of reach without the password is refused.
    let no_password = edit(json!({ "target_pattern": "bob" })).await;
    assert_eq!(
        no_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_password.text
    );
    // With the password, and with an expiry in the 30 days, the wide rule is accepted.
    let ok = edit(json!({ "host": null, "expires_in_minutes": 60, "password": PASSWORD })).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.body["covers_every_host"], true);
    assert_eq!(ok.body["covers_every_target"], false);
}
