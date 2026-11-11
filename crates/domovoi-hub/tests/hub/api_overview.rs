//! The overview counts and the host filter of the action list.

use axum::http::StatusCode;
use domovoi_core::ports::{ActionRepo, HostRepo, IncidentRepo, Presence};
use domovoi_core::{AgentIdentity, Risk, Verdict};
use domovoi_testkit::build::{action, host, incident, open_incident, resolved_incident};
use serde_json::json;

use crate::api_harness::{Api, api};

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
async fn overview_counts_incidents_approvals_and_offline_hosts() {
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
    // On media: one open incident and one proposed action.
    let other = open_incident(media.id, "disk.usage", None).unwrap();
    IncidentRepo::insert(&*a.store, &other).await.unwrap();
    let on_media = action(other.id(), media.id, Risk::Write).unwrap();
    ActionRepo::insert(&*a.store, &on_media).await.unwrap();
    a.propose(Risk::Write).await;
    a.propose(Risk::Write).await;

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
        all.body,
        json!({
            "incidents_action_needed": 2,
            "incidents_open": 4,
            "approvals_waiting": 3,
            "hosts_offline": 1,
            "hosts_total": 2,
        })
    );

    let nas = a.get("/api/v1/overview?host=nas", &login).await;
    assert_eq!(
        nas.body,
        json!({
            "incidents_action_needed": 1,
            "incidents_open": 3,
            "approvals_waiting": 2,
            "hosts_offline": 0,
            "hosts_total": 1,
        })
    );
    let media = a.get("/api/v1/overview?host=media", &login).await;
    assert_eq!(media.body["approvals_waiting"], 1);
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
async fn action_list_filters_by_host() {
    let a = api().await;
    let login = a.admin().await;
    let media = host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &media).await.unwrap();
    let other = open_incident(media.id, "disk.usage", None).unwrap();
    IncidentRepo::insert(&*a.store, &other).await.unwrap();
    let on_media = action(other.id(), media.id, Risk::Write).unwrap();
    ActionRepo::insert(&*a.store, &on_media).await.unwrap();
    let on_nas = a.propose(Risk::Write).await;

    let everything = a.get("/api/v1/actions?status=proposed", &login).await;
    assert_eq!(everything.body["items"].as_array().unwrap().len(), 2);

    let media_only = a
        .get("/api/v1/actions?status=proposed&host=media", &login)
        .await;
    assert_eq!(media_only.status, StatusCode::OK, "{}", media_only.text);
    let items = media_only.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], on_media.id().to_string());

    let nas_only = a.get("/api/v1/actions?host=nas", &login).await;
    assert_eq!(nas_only.body["items"][0]["id"], on_nas.id().to_string());
    assert_eq!(
        a.get("/api/v1/actions?host=nowhere", &login).await.status,
        StatusCode::NOT_FOUND
    );
}
