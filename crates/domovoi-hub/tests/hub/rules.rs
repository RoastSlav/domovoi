use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api};

/// The valid body of a rule, with one field replaced by `value`.
fn rule_body(field: &str, value: Value) -> Value {
    let mut body = json!({
        "host": "nas",
        "check": "disk.usage",
        "target_pattern": "pool*",
        "predicates": [],
        "note": "a known condition",
        "expires_in_minutes": 60,
    });
    body[field] = value;
    body
}

async fn created_rule(a: &Api, login: &Login, body: Value) -> Value {
    let reply = a.post("/api/v1/ack-rules", login, body).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    reply.body
}

async fn listed_rules(a: &Api, login: &Login) -> Vec<Value> {
    let reply = a.get("/api/v1/ack-rules", login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body.as_array().unwrap().clone()
}

#[tokio::test]
async fn creates_lists_and_deletes_a_known_condition() {
    let a = api().await;
    let login = a.admin().await;
    assert_eq!(
        a.send(Method::GET, "/api/v1/ack-rules", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED,
        "the list needs a session"
    );
    assert!(listed_rules(&a, &login).await.is_empty());

    let nightly = created_rule(
        &a,
        &login,
        json!({
            "host": "nas",
            "check": "disk.usage",
            "target_pattern": "pool*",
            "predicates": [{ "key": "used_pct", "op": "at_most", "value": "90" }],
            "note": "the nightly backup fills the pool to 90%",
            "expires_in_minutes": 60,
        }),
    )
    .await;
    assert_eq!(nightly["host"], "nas");
    assert_eq!(nightly["check"], "disk.usage");
    assert_eq!(nightly["target_pattern"], "pool*");
    assert_eq!(nightly["predicates"][0]["key"], "used_pct");
    assert_eq!(nightly["predicates"][0]["op"], "at_most");
    assert_eq!(nightly["predicates"][0]["value"], "90");
    assert_eq!(
        nightly["expires_at"], "2026-10-31T13:00:00Z",
        "60 minutes from the fake clock"
    );

    let ssh = created_rule(
        &a,
        &login,
        json!({
            "host": null,
            "check": "security.ssh_login",
            "target_pattern": null,
            "predicates": [{ "key": "user", "op": "equals", "value": "root" }],
            "note": "the deploy key logs in as root from the build box",
            "expires_in_minutes": null,
        }),
    )
    .await;
    assert_eq!(ssh["host"], Value::Null, "no host means every host");
    assert_eq!(
        ssh["expires_at"],
        Value::Null,
        "no minutes means never expires"
    );

    let rules = listed_rules(&a, &login).await;
    assert_eq!(
        rules.len(),
        2,
        "expired rules are listed too, in time order"
    );
    assert_eq!(
        rules[0]["id"], nightly["id"],
        "the rule expiring first comes first"
    );
    assert_eq!(
        rules[1]["id"], ssh["id"],
        "a rule that never expires comes last"
    );

    let created = a.audit_events("ack_rule.created").await;
    assert_eq!(created.len(), 2, "one event per rule");
    assert!(created[0].actor.starts_with("user:"));
    assert_eq!(
        created[0].subject,
        format!("ack_rule:{}", ssh["id"].as_str().unwrap()),
        "newest first"
    );

    let id = nightly["id"].as_str().unwrap();
    let deleted = a
        .send(
            Method::DELETE,
            &format!("/api/v1/ack-rules/{id}"),
            Some(&login),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text);
    let rules = listed_rules(&a, &login).await;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], ssh["id"]);

    let deleted_events = a.audit_events("ack_rule.deleted").await;
    assert_eq!(deleted_events.len(), 1);
    assert_eq!(
        deleted_events[0].subject,
        format!("ack_rule:{id}"),
        "the audit names the rule that went away"
    );
    assert_eq!(
        deleted_events[0].detail["note"],
        "the nightly backup fills the pool to 90%"
    );

    assert_eq!(
        a.send(
            Method::DELETE,
            "/api/v1/ack-rules/0196ac01-0000-7000-8000-000000000000",
            Some(&login),
            None,
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "a rule that is not there"
    );
    assert_eq!(
        a.send(
            Method::DELETE,
            "/api/v1/ack-rules/not-an-id",
            Some(&login),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "an id that is not a uuid"
    );
    assert_eq!(
        listed_rules(&a, &login).await.len(),
        1,
        "the ssh rule is still there"
    );
}

#[tokio::test]
async fn invalid_rule_fields_are_refused() {
    let a = api().await;
    let login = a.admin().await;
    let cases: Vec<(Value, &str)> = [
        ("host", json!("nowhere")),
        ("check", json!("Disk.Usage")),
        ("target_pattern", json!("pool*extra*")),
        ("note", json!("")),
        ("expires_in_minutes", json!(0)),
        ("expires_in_minutes", json!(525_601)),
    ]
    .into_iter()
    .map(|(field, value)| (rule_body(field, value), field))
    .chain([
        (
            rule_body(
                "predicates",
                json!([{ "key": "Used Pct", "op": "at_most", "value": "90" }]),
            ),
            "evidence key",
        ),
        (
            rule_body(
                "predicates",
                json!([{ "key": "used_pct", "op": "at_most", "value": "many" }]),
            ),
            "at_most",
        ),
        (
            rule_body(
                "predicates",
                json!([{ "key": "user", "op": "contains", "value": "" }]),
            ),
            "contains",
        ),
        (
            rule_body(
                "predicates",
                Value::Array(
                    (0..11)
                        .map(|i| json!({ "key": format!("k{i}"), "op": "equals", "value": "x" }))
                        .collect(),
                ),
            ),
            "conditions",
        ),
    ])
    .collect();
    for (body, needle) in cases {
        let reply = a.post("/api/v1/ack-rules", &login, body).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "a {needle} problem should be refused: {}",
            reply.text
        );
        let message = reply.body["message"].as_str().unwrap_or_default();
        let needle = match needle {
            "host" => "unknown host",
            "check" => "check key",
            "target_pattern" => "target pattern",
            "note" => "note",
            "expires_in_minutes" => "minutes",
            other => other,
        };
        assert!(
            message.contains(needle),
            "the message says what is wrong, not just {needle}: {message}"
        );
    }
    assert!(
        listed_rules(&a, &login).await.is_empty(),
        "nothing refused was stored"
    );
    assert!(
        a.audit_events("ack_rule.created").await.is_empty(),
        "a refused write is not audited"
    );
}

#[tokio::test]
async fn creating_a_rule_without_the_csrf_token_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/ack-rules")
        .header(header::COOKIE, &login.cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "host": "nas",
                "check": "disk.usage",
                "target_pattern": null,
                "predicates": [],
                "note": "written without the CSRF token",
                "expires_in_minutes": 60,
            })
            .to_string(),
        ))
        .unwrap();
    let reply = a.raw(req).await;
    assert_eq!(
        reply.status,
        StatusCode::FORBIDDEN,
        "the CSRF guard runs first: {}",
        reply.text
    );
    assert!(
        listed_rules(&a, &login).await.is_empty(),
        "the rule was not stored"
    );
    assert_eq!(
        a.send(
            Method::DELETE,
            "/api/v1/ack-rules/0196ac01-0000-7000-8000-000000000000",
            Some(&login),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "deleting needs the CSRF token too"
    );
}
