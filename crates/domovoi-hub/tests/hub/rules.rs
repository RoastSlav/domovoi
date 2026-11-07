use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use domovoi_core::ports::{AckRepo, IncidentRepo};
use domovoi_core::{
    AckRule, AckRuleId, CheckKey, Evidence, EvidenceValue, HostId, Severity, Signal, SourceKind,
    TargetKey,
};
use domovoi_engine::{DomainEvent, EventKind};
use serde_json::{Value, json};
use tokio::sync::broadcast;

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

/// A firing warning for `check` on the harness's `nas` host, now, with `evidence`.
fn firing(a: &Api, check: &str, evidence: Vec<(&str, EvidenceValue)>) -> Signal {
    let mut all = Evidence::new();
    for (key, value) in evidence {
        all.insert(key, value);
    }
    Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse(check).unwrap(),
        None,
        Severity::Warning,
        format!("{check} firing"),
        a.now(),
    )
    .with_evidence(all)
}

/// A successful SSH login by `user` from `ip`, firing on the harness's `nas` now.
fn ssh_login(a: &Api, user: &str, ip: &str, network: &str) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert("ip", EvidenceValue::Text(ip.to_owned()));
    evidence.insert("network", EvidenceValue::Text(network.to_owned()));
    evidence.insert("count", EvidenceValue::Int(1));
    Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse("security.ssh_login").unwrap(),
        Some(TargetKey::parse(user).unwrap()),
        Severity::Warning,
        format!("ssh login by {user} from {ip}"),
        a.now(),
    )
    .with_evidence(evidence)
}

/// The next incident the manager confirms, skipping the triage request that comes with it.
/// Two firing signals in one ingest confirm it: the default policy needs two.
async fn next_opened(events: &mut broadcast::Receiver<DomainEvent>) -> DomainEvent {
    loop {
        let event = events.recv().await.expect("the manager publishes events");
        if event.kind == EventKind::IncidentOpened {
            return event;
        }
    }
}

/// The next change to an incident, triage requests skipped. A signal for an incident that is
/// already open changes it, so no `IncidentOpened` follows.
async fn next_change(events: &mut broadcast::Receiver<DomainEvent>) -> DomainEvent {
    loop {
        let event = events.recv().await.expect("the manager publishes events");
        if event.kind != EventKind::TriageRequested {
            return event;
        }
    }
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
                json!([{ "key": "ip", "op": "in_network", "value": "192.168.1.0/33" }]),
            ),
            "address range",
        ),
        (
            rule_body(
                "predicates",
                json!([{ "key": "ip", "op": "in_network", "value": "not a range" }]),
            ),
            "address range",
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

#[tokio::test]
async fn a_rule_whose_host_is_gone_is_still_listed_and_deletable() {
    let a = api().await;
    let login = a.admin().await;
    let ghost = HostId::new();
    let rule = AckRule {
        id: AckRuleId::new(),
        host: Some(ghost),
        check: CheckKey::parse("disk.usage").unwrap(),
        target_pattern: None,
        predicates: Vec::new(),
        note: "the host row was edited away by hand".to_owned(),
        expires_at: None,
    };
    AckRepo::put_rule(&*a.store, &rule).await.unwrap();

    let rules = listed_rules(&a, &login).await;
    assert_eq!(
        rules.len(),
        1,
        "a rule with a broken host reference cannot fail the whole list"
    );
    assert_eq!(
        rules[0]["host"],
        format!("unknown host {ghost}"),
        "the row names the host that is missing"
    );

    let deleted = a
        .send(
            Method::DELETE,
            &format!("/api/v1/ack-rules/{}", rule.id),
            Some(&login),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text);
    assert!(
        listed_rules(&a, &login).await.is_empty(),
        "the rule can be deleted from the page"
    );
}

#[tokio::test]
async fn a_matching_known_condition_silences_the_next_incident() {
    let a = api().await;
    let login = a.admin().await;
    let mut events = a.bus.subscribe();

    created_rule(
        &a,
        &login,
        json!({
            "host": "nas",
            "check": "disk.usage",
            "target_pattern": null,
            "predicates": [{ "key": "used_pct", "op": "at_most", "value": "90" }],
            "note": "the nightly backup fills the pool",
            "expires_in_minutes": 60,
        }),
    )
    .await;

    // One firing keeps a normal incident pending; the second confirms it, which is
    // the point where a card would go out.
    a.manager
        .ingest(vec![
            firing(&a, "disk.usage", vec![("used_pct", EvidenceValue::Int(85))]),
            firing(&a, "disk.usage", vec![("used_pct", EvidenceValue::Int(85))]),
        ])
        .await
        .unwrap();
    let opened = next_opened(&mut events).await;
    assert_eq!(
        opened.incident.check().as_str(),
        "disk.usage",
        "the signal the rule is about"
    );
    assert!(
        !opened.notify,
        "a matching rule holds the card back: {opened:?}"
    );
    assert!(
        IncidentRepo::get(&*a.store, opened.incident.id())
            .await
            .unwrap()
            .is_some(),
        "the incident is stored, only the notification is held back"
    );

    a.manager
        .ingest(vec![
            firing(&a, "security.ssh_login", vec![]),
            firing(&a, "security.ssh_login", vec![]),
        ])
        .await
        .unwrap();
    let opened = next_opened(&mut events).await;
    assert_eq!(
        opened.incident.check().as_str(),
        "security.ssh_login",
        "the rule covers one check, not every signal"
    );
    assert!(
        opened.notify,
        "a signal the rule does not cover still gets a card"
    );
}

#[tokio::test]
async fn a_login_from_an_expected_address_range_never_alerts() {
    let a = api().await;
    let login = a.admin().await;
    let mut events = a.bus.subscribe();

    let expected = created_rule(
        &a,
        &login,
        json!({
            "host": "nas",
            "check": "security.ssh_login",
            "target_pattern": "alice",
            "predicates": [{ "key": "ip", "op": "in_network", "value": "192.168.1.0/24" }],
            "note": "alice logs in from the office",
            "expires_in_minutes": null,
        }),
    )
    .await;
    assert_eq!(
        expected["predicates"][0]["op"], "in_network",
        "the address range comes back as it was sent"
    );
    assert_eq!(expected["predicates"][0]["value"], "192.168.1.0/24");

    // Two logins inside the range: the incident is kept, and no card goes out.
    a.manager
        .ingest(vec![
            ssh_login(&a, "alice", "192.168.1.20", "lan"),
            ssh_login(&a, "alice", "192.168.1.20", "lan"),
        ])
        .await
        .unwrap();
    let opened = next_opened(&mut events).await;
    assert_eq!(
        opened.incident.check().as_str(),
        "security.ssh_login",
        "the login the rule is about"
    );
    assert!(
        !opened.notify,
        "an expected login is recorded without a card: {opened:?}"
    );

    // The same user from outside the range is not expected, so the card goes out.
    a.manager
        .ingest(vec![
            ssh_login(&a, "alice", "203.0.113.9", "wan"),
            ssh_login(&a, "alice", "203.0.113.9", "wan"),
        ])
        .await
        .unwrap();
    let change = next_change(&mut events).await;
    assert_eq!(
        change.incident.check().as_str(),
        "security.ssh_login",
        "the login the rule is about"
    );
    assert!(
        change.notify,
        "a login from outside the expected range alerts"
    );
}
