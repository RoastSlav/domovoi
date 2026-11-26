//! The overview counts and the group and text filters of the incident list.

use axum::http::StatusCode;
use domovoi_core::ports::{HostRepo, IncidentRepo, Presence};
use domovoi_core::{AgentIdentity, Verdict};
use domovoi_testkit::build::{host, incident, open_incident, resolved_incident};
use serde_json::json;

use crate::api_harness::{Api, api, api_with_operations};
use domovoi_core::Actor;

/// Enrolls an agent for `host`, as a finished enrollment leaves it in the store.
async fn enroll(a: &Api, host: domovoi_core::HostId) {
    let agent = AgentIdentity {
        agent: domovoi_core::AgentId::new(),
        host,
        public_key: [0; 32],
        version: "0.0.1".to_owned(),
        enrolled_at: a.now(),
        revoked_at: None,
    };
    HostRepo::put_agent(&*a.store, &agent).await.unwrap();
}

#[tokio::test]
async fn overview_counts_incidents_and_offline_hosts() {
    let a = api().await;
    let login = a.admin().await;
    let media = host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &media).await.unwrap();

    // On nas: the harness's open incident, an expected one, a pending one and a resolved one.
    let mut expected = open_incident(a.nas.id, "cpu.load", None).unwrap();
    expected.set_verdict(Some(Verdict::Expected));
    IncidentRepo::insert(&*a.store, &expected).await.unwrap();
    let pending = incident(a.nas.id, "mem.used", None).unwrap();
    IncidentRepo::insert(&*a.store, &pending).await.unwrap();
    let done = resolved_incident(a.nas.id, "net.ping", None, a.now()).unwrap();
    IncidentRepo::insert(&*a.store, &done).await.unwrap();
    // On media: one open incident.
    let other = open_incident(media.id, "disk.usage", None).unwrap();
    IncidentRepo::insert(&*a.store, &other).await.unwrap();

    // nas has an agent that is connected; media has one that is not.
    enroll(&a, a.nas.id).await;
    enroll(&a, media.id).await;
    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: "0.0.1".to_owned(),
        },
    );

    let all = a.get("/api/v1/overview", &login).await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.text);
    assert_eq!(
        without_position(all.body),
        json!({
            "incidents_action_needed": 2,
            "incidents_open": 4,
            "incident_groups": {
                "needs_action": 2, "monitor": 0, "pending": 1, "expected": 1, "handled": 0,
            },
            "hosts_offline": 1,
            "hosts_total": 2,
            "operations_running": 0, "operations_waiting_approval": 0, "operations_waiting_user": 0,
            "operations_paused": 0, "attention_total": 3,
        })
    );

    let nas = a.get("/api/v1/overview?host=nas", &login).await;
    assert_eq!(
        without_position(nas.body),
        json!({
            "incidents_action_needed": 1,
            "incidents_open": 3,
            "incident_groups": {
                "needs_action": 1, "monitor": 0, "pending": 1, "expected": 1, "handled": 0,
            },
            "hosts_offline": 0,
            "hosts_total": 1,
            "operations_running": 0, "operations_waiting_approval": 0, "operations_waiting_user": 0,
            "operations_paused": 0, "attention_total": 1,
        })
    );
    let media = a.get("/api/v1/overview?host=media", &login).await;
    assert_eq!(media.body["hosts_offline"], 1);
}

#[tokio::test]
async fn overview_does_not_count_a_host_without_an_agent_as_offline() {
    let a = api().await;
    let login = a.admin().await;

    let reply = a.get("/api/v1/overview", &login).await;

    assert_eq!(reply.body["hosts_total"], 1);
    assert_eq!(
        reply.body["hosts_offline"], 0,
        "never enrolled, so not offline"
    );
}

#[tokio::test]
async fn overview_needs_a_session_and_a_known_host() {
    let a = api().await;
    let login = a.admin().await;

    let anonymous = a
        .send(axum::http::Method::GET, "/api/v1/overview", None, None)
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        a.get("/api/v1/overview?host=nowhere", &login).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn incident_list_filters_by_group_and_text() {
    let a = api().await;
    let login = a.admin().await;
    let mut watched = open_incident(a.nas.id, "cpu.load", None).unwrap();
    watched.set_verdict(Some(Verdict::Monitor));
    IncidentRepo::insert(&*a.store, &watched).await.unwrap();
    let mut known = open_incident(a.nas.id, "mem.used", Some("swap")).unwrap();
    known.set_verdict(Some(Verdict::Expected));
    IncidentRepo::insert(&*a.store, &known).await.unwrap();

    let ids = async |query: &str| -> Vec<String> {
        let reply = a.get(&format!("/api/v1/incidents?{query}"), &login).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        reply.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect()
    };

    assert_eq!(ids("group=monitor").await, [watched.id().to_string()]);
    assert_eq!(ids("group=expected").await, [known.id().to_string()]);
    // The harness's own open incident has no verdict, so it needs action.
    assert_eq!(ids("group=needs_action").await.len(), 1);
    assert!(ids("group=resolved").await.is_empty());
    assert_eq!(ids("q=SWAP").await, [known.id().to_string()]);
    assert_eq!(ids("q=cpu.load&group=monitor").await.len(), 1);
    assert!(ids("q=cpu.load&group=expected").await.is_empty());
    assert_eq!(ids("q=%20%20").await.len(), 3, "blank text filters nothing");

    let unknown = a.get("/api/v1/incidents?group=open", &login).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}

/// The counts without the journal position they were read at, which moves with every write.
fn without_position(mut body: serde_json::Value) -> serde_json::Value {
    if let Some(object) = body.as_object_mut() {
        object.remove("as_of_seq");
    }
    body
}

#[tokio::test]
async fn overview_counts_operations_by_state() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let waiting = a.create_operation("waiting", true).await;
    a.propose_restart(waiting).await;
    let other = a.create_operation("another waiting", false).await;
    a.propose_restart(other).await;
    let paused = a.create_operation("paused", false).await;
    a.ops()
        .pause(paused, Actor::System("tests".to_owned()))
        .await
        .unwrap();

    let reply = a.get("/api/v1/overview", &login).await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["operations_waiting_approval"], 2);
    assert_eq!(reply.body["operations_paused"], 1);
    assert_eq!(reply.body["operations_running"], 0);
    assert_eq!(reply.body["operations_waiting_user"], 0);
}

#[tokio::test]
async fn attention_total_is_the_sum_of_what_the_attention_centre_lists() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = a.create_operation("waiting", true).await;
    a.propose_restart(op).await;
    let mut monitor = open_incident(a.nas.id, "cpu.load", None).unwrap();
    monitor.set_verdict(Some(Verdict::Expected));
    IncidentRepo::insert(&*a.store, &monitor).await.unwrap();

    let body = a.get("/api/v1/overview", &login).await.body;

    let n = |k: &str| body[k].as_u64().unwrap();
    let expected = body["incident_groups"]["needs_action"].as_u64().unwrap()
        + n("operations_waiting_approval")
        + n("operations_waiting_user")
        + n("operations_paused")
        + n("hosts_offline");
    assert_eq!(n("attention_total"), expected);
    assert!(expected >= 2, "{body}");
}

#[tokio::test]
async fn attention_counts_follow_the_host_filter() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let media = host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &media).await.unwrap();
    let op = a.create_operation("on nas", false).await;
    a.propose_restart(op).await;

    let nas = a.get("/api/v1/overview?host=nas", &login).await.body;
    let other = a.get("/api/v1/overview?host=media", &login).await.body;

    assert_eq!(nas["operations_waiting_approval"], 1);
    assert_eq!(other["operations_waiting_approval"], 0);
    assert_eq!(other["attention_total"], 0);
    assert_eq!(
        a.get("/api/v1/overview?host=nope", &login).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn operation_counts_are_zero_when_orchestration_is_off() {
    let a = api().await;
    let login = a.admin().await;

    let body = a.get("/api/v1/overview", &login).await.body;

    for key in [
        "operations_running",
        "operations_waiting_approval",
        "operations_waiting_user",
        "operations_paused",
    ] {
        assert_eq!(body[key], 0, "{key}");
    }
}
