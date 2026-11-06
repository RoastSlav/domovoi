//! Dismissing an incident as a known condition, over the API.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use domovoi_core::ports::{AckRepo, IncidentRepo};
use domovoi_core::{
    AckRule, CheckKey, Incident, IncidentId, IncidentStatus, Severity, Signal, SignalClass,
    SourceKind, TargetKey, TargetPattern, Verdict,
};
use domovoi_engine::EventKind;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Api, api};

/// What a human writes when dismissing the harness's disk warning.
const REASON: &str = "the nightly backup fills the pool to 90%, and that is fine";

fn dismiss_uri(incident: &Incident) -> String {
    format!("/api/v1/incidents/{}/dismiss", incident.id())
}

/// The incident as the store now holds it.
async fn stored(a: &Api, incident: &Incident) -> Incident {
    IncidentRepo::get(&*a.store, incident.id())
        .await
        .unwrap()
        .unwrap()
}

async fn stored_rules(a: &Api) -> Vec<AckRule> {
    AckRepo::list_rules(&*a.store).await.unwrap()
}

/// The same disk warning the harness's incident is about, firing now.
fn disk_warning(a: &Api) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse("disk.usage").unwrap(),
        Some(TargetKey::parse("/srv").unwrap()),
        Severity::Warning,
        "disk.usage firing".to_owned(),
        a.now(),
    )
}

#[tokio::test]
async fn dismissing_with_zero_minutes_resolves_the_incident_and_creates_a_rule_that_never_expires()
{
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .post(
            &dismiss_uri(&a.incident),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 0 }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);

    let incident = stored(&a, &a.incident).await;
    assert_eq!(incident.status(), IncidentStatus::Resolved);
    assert_eq!(
        incident.verdict(),
        Some(Verdict::Expected),
        "the verdict says this condition is expected"
    );
    assert_eq!(
        incident.resolved_at(),
        Some(a.now()),
        "dismissed on the spot"
    );

    let rules = stored_rules(&a).await;
    assert_eq!(rules.len(), 1, "one rule for the dismissed check");
    let rule = &rules[0];
    assert_eq!(rule.host, Some(a.nas.id), "the rule is for this host");
    assert_eq!(rule.check.as_str(), "disk.usage");
    assert_eq!(
        rule.target_pattern,
        Some(TargetPattern::parse("/srv").unwrap()),
        "the rule matches this target exactly, so other disks keep alerting"
    );
    assert_eq!(
        rule.note, REASON,
        "the rule keeps the reason for a human to read"
    );
    assert_eq!(
        rule.expires_at, None,
        "0 minutes means the rule never expires"
    );
    assert!(rule.predicates.is_empty(), "no evidence conditions");

    let created = a.audit_events("ack_rule.created").await;
    assert_eq!(created.len(), 1, "the rule is audited once");
    assert_eq!(created[0].subject, format!("ack_rule:{}", rule.id));
    assert_eq!(created[0].detail["note"], REASON);
    assert_eq!(created[0].detail["expires_at"], Value::Null);
}

#[tokio::test]
async fn dismissing_for_a_period_expires_the_rule() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .post(
            &dismiss_uri(&a.incident),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 1440 }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);

    let rules = stored_rules(&a).await;
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].expires_at,
        Some(a.now() + SignedDuration::from_mins(1440)),
        "the rule goes away after the chosen period"
    );
    assert_eq!(rules[0].note, REASON);
}

#[tokio::test]
async fn dismissing_without_a_period_resolves_the_incident_and_stores_no_rule() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .post(
            &dismiss_uri(&a.incident),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": null }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);

    assert!(
        stored_rules(&a).await.is_empty(),
        "no period means no known condition, so the check alerts again"
    );
    assert!(
        a.audit_events("ack_rule.created").await.is_empty(),
        "a dismiss that stored no rule is not audited as a rule"
    );

    let incident = stored(&a, &a.incident).await;
    assert_eq!(incident.status(), IncidentStatus::Resolved);
    assert_eq!(incident.verdict(), Some(Verdict::Expected));
    let timeline = IncidentRepo::timeline(&*a.store, incident.id())
        .await
        .unwrap();
    let kinds: Vec<&str> = timeline.iter().map(|entry| entry.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["human.dismissed", "status.changed"],
        "the reason is recorded, then the close"
    );
    assert!(
        timeline[0].summary.contains(REASON),
        "the reason is in the line: {}",
        timeline[0].summary
    );
    assert!(
        timeline[0].summary.starts_with("dismissed by user:"),
        "the line says who dismissed it: {}",
        timeline[0].summary
    );
}

#[tokio::test]
async fn dismissing_a_check_keeps_it_quiet_when_it_fires_again() {
    let a = api().await;
    let login = a.admin().await;
    let mut events = a.bus.subscribe();
    let reply = a
        .post(
            &dismiss_uri(&a.incident),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 0 }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    let closed = events.recv().await.unwrap();
    assert_eq!(
        closed.kind,
        EventKind::IncidentResolved,
        "the close is published"
    );

    let warning = disk_warning(&a);
    let again = Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse("disk.usage").unwrap(),
        Some(TargetKey::parse("/srv").unwrap()),
        Severity::Warning,
        "disk.usage firing again".to_owned(),
        a.now() + SignedDuration::from_mins(1),
    );
    a.manager
        .ingest(vec![warning.clone(), again])
        .await
        .unwrap();
    let mut events_seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        events_seen.push(event);
    }
    assert!(
        events_seen
            .iter()
            .any(|event| event.kind == EventKind::IncidentReopened),
        "the check fires again, so the incident reopens: {events_seen:?}"
    );
    assert!(
        events_seen.iter().all(|event| !event.notify),
        "a known condition reopens quietly: {events_seen:?}"
    );
    assert!(
        !events_seen
            .iter()
            .any(|event| event.kind == EventKind::IncidentOpened),
        "no new incident is opened for the known condition: {events_seen:?}"
    );
    let reopened = IncidentRepo::find_active_by_fingerprint(&*a.store, &warning.fingerprint())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reopened.verdict(),
        Some(Verdict::Expected),
        "the reopened incident keeps the verdict a human gave it"
    );
    let rules = stored_rules(&a).await;
    assert_eq!(rules.len(), 1, "the rule stays, so the check stays quiet");
}

#[tokio::test]
async fn dismissing_refuses_an_empty_reason_and_too_many_minutes() {
    let a = api().await;
    let login = a.admin().await;
    let bodies = [
        json!({ "reason": "", "ignore_for_minutes": 0 }),
        json!({ "reason": "   ", "ignore_for_minutes": 0 }),
        json!({ "reason": "x".repeat(501), "ignore_for_minutes": 0 }),
        json!({ "reason": REASON, "ignore_for_minutes": 525_601 }),
    ];
    for body in bodies {
        let reply = a.post(&dismiss_uri(&a.incident), &login, body).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "refused: {}",
            reply.text
        );
        assert!(
            reply.body["message"]
                .as_str()
                .unwrap_or_default()
                .contains("reason")
                || reply.body["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("minutes"),
            "the message says what is wrong: {}",
            reply.text
        );
    }

    assert_eq!(
        stored(&a, &a.incident).await.status(),
        IncidentStatus::Open,
        "nothing was closed"
    );
    assert!(stored_rules(&a).await.is_empty(), "nothing was stored");
    assert!(
        a.audit_events("ack_rule.created").await.is_empty(),
        "a refused dismiss is not audited"
    );
}

#[tokio::test]
async fn dismissing_a_target_with_a_star_is_refused_before_anything_is_stored() {
    let a = api().await;
    let login = a.admin().await;
    // A target key allows `*` anywhere, so this condition has no rule pattern that matches
    // it exactly. A pattern for it would silence more than this one condition.
    // `Immediate` opens the incident at once, as a check that reports a hard failure does.
    let signal = Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse("disk.usage").unwrap(),
        Some(TargetKey::parse("a*b").unwrap()),
        Severity::Warning,
        "disk.usage firing on a*b".to_owned(),
        a.now(),
    )
    .with_class(SignalClass::Immediate);
    a.manager.ingest(vec![signal.clone()]).await.unwrap();
    let starred = IncidentRepo::find_active_by_fingerprint(&*a.store, &signal.fingerprint())
        .await
        .unwrap()
        .unwrap();

    let reply = a
        .post(
            &dismiss_uri(&starred),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 1440 }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("target"),
        "the message names the target: {}",
        reply.text
    );
    assert_eq!(
        stored(&a, &starred).await.status(),
        IncidentStatus::Open,
        "the incident is not closed by a refused dismiss"
    );
    assert!(stored_rules(&a).await.is_empty(), "no rule was stored");

    // Without a period, dismissing needs no rule, so the same incident can be dismissed.
    let reply = a
        .post(
            &dismiss_uri(&starred),
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": null }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    assert_eq!(
        stored(&a, &starred).await.status(),
        IncidentStatus::Resolved,
        "the dismiss closed it"
    );
    assert!(
        stored_rules(&a).await.is_empty(),
        "no rule, so no wildcards"
    );
}

#[tokio::test]
async fn dismissing_an_incident_that_is_not_there_is_not_found() {
    let a = api().await;
    let login = a.admin().await;
    let unknown = IncidentId::new();
    for uri in [
        format!("/api/v1/incidents/{unknown}/dismiss"),
        "/api/v1/incidents/not-an-id/dismiss".to_owned(),
    ] {
        let reply = a
            .post(
                &uri,
                &login,
                json!({ "reason": REASON, "ignore_for_minutes": 0 }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}: {}", reply.text);
    }
    assert!(stored_rules(&a).await.is_empty(), "a 404 stores no rule");
}

#[tokio::test]
async fn dismissing_the_same_incident_twice_is_not_found() {
    let a = api().await;
    let login = a.admin().await;
    let uri = dismiss_uri(&a.incident);
    let first = a
        .post(
            &uri,
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 0 }),
        )
        .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.text);

    let second = a
        .post(
            &uri,
            &login,
            json!({ "reason": REASON, "ignore_for_minutes": 0 }),
        )
        .await;
    assert_eq!(
        second.status,
        StatusCode::NOT_FOUND,
        "the incident is closed, so there is nothing to dismiss: {}",
        second.text
    );
    assert_eq!(
        stored_rules(&a).await.len(),
        1,
        "the rule created by the first dismiss is kept"
    );
    assert_eq!(
        a.audit_events("ack_rule.created").await.len(),
        1,
        "the refused dismiss created no rule to audit"
    );
}

#[tokio::test]
async fn dismissing_needs_a_session_and_the_csrf_token() {
    let a = api().await;
    let login = a.admin().await;
    let uri = dismiss_uri(&a.incident);
    let body = json!({ "reason": REASON, "ignore_for_minutes": 0 });
    let anonymous = a.send(Method::POST, &uri, None, Some(body.clone())).await;
    assert_eq!(
        anonymous.status,
        StatusCode::UNAUTHORIZED,
        "no session: {}",
        anonymous.text
    );

    let req = Request::builder()
        .method(Method::POST)
        .uri(&uri)
        .header(header::COOKIE, &login.cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let no_csrf = a.raw(req).await;
    assert_eq!(
        no_csrf.status,
        StatusCode::FORBIDDEN,
        "the CSRF guard runs before the dismiss: {}",
        no_csrf.text
    );
    assert_eq!(
        stored(&a, &a.incident).await.status(),
        IncidentStatus::Open,
        "the incident is still open"
    );
    assert!(stored_rules(&a).await.is_empty(), "nothing was stored");
}
