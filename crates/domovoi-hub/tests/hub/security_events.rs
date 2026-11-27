//! What the incident page and card say about a blind check.

use axum::http::StatusCode;
use domovoi_core::ports::IncidentRepo;
use domovoi_core::{
    CheckKey, Evidence, EvidenceValue, Incident, Severity, Signal, SignalClass, SourceKind,
    TargetKey,
};
use domovoi_engine::EventKind;
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api_with_triage_and_operations};

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
