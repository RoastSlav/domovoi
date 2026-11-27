//! What the incident page and card say about a blind check, and about security events a
//! verdict has not seen.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use domovoi_core::ports::IncidentRepo;
use domovoi_core::{
    CheckKey, Evidence, EvidenceValue, Incident, Severity, Signal, SignalClass, SourceKind,
    TargetKey, TriageOutcome, TriageReport, Verdict,
};
use domovoi_engine::EventKind;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api, api_with_triage_and_operations};

fn sudo(a: &Api, target: &str, key: &str, text: &str) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert(key, EvidenceValue::Text(text.to_owned()));
    Signal::firing(
        SourceKind::Agent,
        a.nas.id,
        CheckKey::parse("security.sudo_failure").unwrap(),
        Some(TargetKey::parse(target).unwrap()),
        Severity::Warning,
        format!("sudo signal for {target}"),
        a.now(),
    )
    .with_class(SignalClass::Immediate)
    .with_evidence(evidence)
}

async fn open(a: &Api, signal: &Signal) -> Incident {
    a.manager.ingest(vec![signal.clone()]).await.unwrap();
    IncidentRepo::find_active_by_fingerprint(&*a.store, &signal.fingerprint())
        .await
        .unwrap()
        .unwrap()
}

async fn detail(a: &Api, login: &Login, incident: &Incident) -> Value {
    let reply = a
        .get(&format!("/api/v1/incidents/{}", incident.id()), login)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

fn event_lines(detail: &Value) -> Vec<String> {
    detail["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "security.event")
        .map(|entry| entry["summary"].as_str().unwrap().to_owned())
        .collect()
}

fn report() -> TriageOutcome {
    TriageOutcome::Report(TriageReport {
        verdict: Verdict::ActionNeeded,
        headline: "Failed sudo for bob".to_owned(),
        summary: "One failed sudo.".to_owned(),
        cause: "Not established.".to_owned(),
        evidence: Vec::new(),
        next_step: "Check who it was.".to_owned(),
        manual_steps: Vec::new(),
        proposals: Vec::new(),
        details: String::new(),
    })
}

#[tokio::test]
async fn a_blind_check_says_why_and_how_to_fix_it_and_is_not_triaged() {
    let a = api_with_triage_and_operations().await;
    let login = a.admin().await;
    let why = "this agent account may not read the system journal (add it to the \
               systemd-journal group and restart the agent). journalctl said: not seeing";
    let mut events = a.bus.subscribe();
    let incident = open(&a, &sudo(&a, "journal:unreadable", "reason", why)).await;
    let mut kinds = Vec::new();
    while let Ok(event) = events.try_recv() {
        kinds.push(event.kind);
    }
    assert!(kinds.contains(&EventKind::IncidentOpened), "{kinds:?}");
    assert!(!kinds.contains(&EventKind::TriageRequested), "{kinds:?}");

    let body = detail(&a, &login, &incident).await;
    assert_eq!(body["blind"]["why"], why, "{body}");
    let fix = body["blind"]["fix"].as_str().unwrap();
    assert!(fix.contains("usermod -aG systemd-journal"), "{fix}");
    assert!(fix.contains("--only security.sudo_failure"), "{fix}");
    let card = &body["card"];
    assert!(
        card["bullets"][0].as_str().unwrap().contains(why),
        "the whole reason is on the card: {card}"
    );
    assert_eq!(card["next_step"], fix);
    assert!(event_lines(&body).is_empty(), "blindness is not an event");

    let reply = a
        .post(
            &format!("/api/v1/incidents/{}/triage", incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert!(reply.text.contains("cannot read"), "{}", reply.text);
}

#[tokio::test]
async fn a_reopened_incident_is_no_longer_dismissed_and_shows_the_event() {
    let a = api().await;
    let login = a.admin().await;
    let incident = open(&a, &sudo(&a, "bob", "example", "first")).await;
    let reply = a
        .post(
            &format!("/api/v1/incidents/{}/dismiss", incident.id()),
            &login,
            json!({ "reason": "me", "ignore_for_minutes": 1 }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    let dismissed = detail(&a, &login, &incident).await;
    assert!(dismissed["known_condition"].is_string(), "{dismissed}");

    a.advance(SignedDuration::from_mins(3));
    a.manager
        .ingest(vec![sudo(&a, "bob", "example", "second try")])
        .await
        .unwrap();

    let body = detail(&a, &login, &incident).await;
    assert_eq!(body["incident"]["status"], "open", "{body}");
    assert_eq!(body["known_condition"], Value::Null, "{body}");
    let lines = event_lines(&body);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("example: second try"), "{lines:?}");
}

#[tokio::test]
async fn events_after_a_report_are_listed_and_the_verdict_says_it_does_not_cover_them() {
    let a = api().await;
    let login = a.admin().await;
    let incident = open(&a, &sudo(&a, "bob", "example", "first")).await;
    a.advance(SignedDuration::from_secs(60));
    a.manager
        .attach_triage(incident.id(), report(), BTreeMap::new())
        .await
        .unwrap();
    let covered = detail(&a, &login, &incident).await;
    assert_eq!(covered["uncovered"], Value::Null, "{covered}");

    a.advance(SignedDuration::from_secs(60));
    a.manager
        .ingest(vec![sudo(&a, "bob", "example", "after the report")])
        .await
        .unwrap();

    let body = detail(&a, &login, &incident).await;
    assert_eq!(body["uncovered"]["events"], 1, "{body}");
    let latest = body["uncovered"]["latest"].as_str().unwrap();
    assert!(latest.contains("after the report"), "{latest}");
    assert_eq!(event_lines(&body).len(), 1, "{body}");
    let first = body["card"]["bullets"][0].as_str().unwrap();
    assert!(
        first.starts_with("Not covered by this verdict: 1 later security event"),
        "{first}"
    );
    assert_eq!(body["report"]["verdict"], covered["report"]["verdict"]);
}

/// An event that arrives while a run is reading is not covered by that run's report.
#[tokio::test]
async fn an_event_during_a_run_is_not_covered_by_its_report() {
    let a = api().await;
    let login = a.admin().await;
    let incident = open(&a, &sudo(&a, "bob", "example", "first")).await;
    a.advance(SignedDuration::from_secs(60));
    let seen = a.now();
    a.advance(SignedDuration::from_secs(60));
    a.manager
        .ingest(vec![sudo(&a, "bob", "example", "while it ran")])
        .await
        .unwrap();
    a.advance(SignedDuration::from_secs(60));
    a.manager
        .attach_triage_seen(incident.id(), report(), BTreeMap::new(), seen)
        .await
        .unwrap();

    let body = detail(&a, &login, &incident).await;
    assert_eq!(body["uncovered"]["events"], 1, "{body}");
}
