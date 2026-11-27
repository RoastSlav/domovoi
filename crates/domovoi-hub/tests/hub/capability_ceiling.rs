//! `GET /api/v1/hosts/{name}/ceiling`: the unattended limits a host reported, read only.

use axum::http::StatusCode;
use domovoi_core::ports::{HostWords, Presence};

use crate::api_harness::{Api, api};

const SUMMARY: &str = r#"{"enabled":true,"hash":"agent-own-hash","entries":[
  {"contract":"firewall.block_source","targets":{"kind":"any"},"max_per_hour":20,"max_active":50,"max_ttl":"1h","args":{"min_prefix_len_v4":24}},
  {"contract":"container.restart","targets":{"kind":"names","names":["plex","jellyfin"]},"max_per_hour":3,"args":{}}]}"#;

fn connect(a: &Api, words: HostWords) {
    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: "0.6.0".to_owned(),
        },
    );
    a.link.set_host_words(a.nas.id, words);
}

fn words(summary: Option<&str>, hash: Option<&str>) -> HostWords {
    HostWords {
        ceiling_summary: summary.map(str::to_owned),
        ceiling_hash: hash.map(str::to_owned),
        ..HostWords::default()
    }
}

#[tokio::test]
async fn ceiling_endpoint_reports_not_reported_when_no_agent_sent_one() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, words(None, None));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["reporting"], "not_reported");
    assert_eq!(reply.body["connected"], true);
    assert_eq!(reply.body["entries"].as_array().map(Vec::len), Some(0));
    assert!(reply.body["hash"].is_null());
}

#[tokio::test]
async fn ceiling_endpoint_returns_the_summary_as_the_agent_reported_it() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, words(Some(SUMMARY), Some("9f86d081")));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let body = &reply.body;
    assert_eq!(body["host"], "nas");
    assert_eq!(body["reporting"], "reported");
    assert_eq!(body["unattended_enabled"], true);
    assert_eq!(body["hash"], "9f86d081");
    assert!(body["unreadable_reason"].is_null());
    let entries = body["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    let fw = &entries[1];
    assert_eq!(fw["targets"]["kind"], "any");
    assert_eq!(fw["max_per_hour"], 20);
    assert_eq!(fw["max_active"], 50);
    assert_eq!(fw["max_ttl_secs"], 3600);
    assert_eq!(fw["args"]["min_prefix_len_v4"], 24);
}

#[tokio::test]
async fn ceiling_entries_are_sorted_by_contract_and_names_ascending() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, words(Some(SUMMARY), None));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    let entries = reply.body["entries"].as_array().expect("entries");
    assert_eq!(entries[0]["contract"], "container.restart");
    assert_eq!(entries[1]["contract"], "firewall.block_source");
    assert_eq!(
        entries[0]["targets"]["names"],
        serde_json::json!(["jellyfin", "plex"])
    );
    assert_eq!(reply.body["hash"], "agent-own-hash");
}

#[tokio::test]
async fn ceiling_endpoint_marks_a_malformed_summary_unreadable_and_never_echoes_it() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, words(Some("{\"marker-secret-text\": nope"), None));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["reporting"], "unreadable");
    let reason = reply.body["unreadable_reason"].as_str().expect("reason");
    assert!(reason.chars().count() <= 200);
    assert!(!reply.text.contains("marker-secret-text"));
    assert_eq!(reply.body["entries"].as_array().map(Vec::len), Some(0));
    assert!(reply.body["unattended_enabled"].is_null());
}

#[tokio::test]
async fn ceiling_endpoint_for_a_host_with_no_agent_is_not_reported_and_not_connected() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["connected"], false);
    assert_eq!(reply.body["reporting"], "not_reported");
}

#[tokio::test]
async fn ceiling_endpoint_requires_a_session() {
    let a = api().await;
    let reply = a
        .send(
            axum::http::Method::GET,
            "/api/v1/hosts/nas/ceiling",
            None,
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ceiling_endpoint_404s_on_an_unknown_host() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/hosts/nope/ceiling", &login).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ceiling_dto_covers_every_field() {
    let a = api().await;
    let login = a.admin().await;
    connect(&a, words(Some(SUMMARY), Some("abc")));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    for field in [
        "host",
        "connected",
        "reporting",
        "unattended_enabled",
        "hash",
        "entries",
        "unreadable_reason",
    ] {
        assert!(reply.body.get(field).is_some(), "{field} missing");
    }
    for field in [
        "contract",
        "targets",
        "max_per_hour",
        "max_active",
        "max_ttl_secs",
        "args",
    ] {
        assert!(
            reply.body["entries"][0].get(field).is_some(),
            "{field} missing"
        );
    }
}

const HELPER_SUMMARY: &str = r#"{"enabled":true,"entries":[
  {"contract":"container.restart","targets":{"kind":"any"},"max_per_hour":3,"args":{}},
  {"contract":"unit.restart","targets":{"kind":"names","names":["nginx.service"]},"max_per_hour":3,"args":{}}]}"#;

fn helper_words(ops: &[&str]) -> HostWords {
    HostWords {
        ceiling_summary: Some(HELPER_SUMMARY.to_owned()),
        helper: domovoi_core::ports::HelperState::Ready,
        helper_ops: ops.iter().map(|op| (*op).to_owned()).collect(),
        ..HostWords::default()
    }
}

#[tokio::test]
async fn a_ceiling_entry_says_whether_the_helper_would_serve_it() {
    let a = api().await;
    let login = a.admin().await;

    // Allowed by the agent's ceiling, but the helper has no `systemd.write` group: the row says so.
    connect(&a, helper_words(&[]));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    let entries = reply.body["entries"].as_array().expect("entries");
    let restart = entries
        .iter()
        .find(|e| e["contract"] == "unit.restart")
        .unwrap();
    assert_eq!(restart["helper"]["state"], "not_offered");
    assert_eq!(restart["helper"]["reason"], "helper_systemd_group_missing");
    assert!(
        restart["helper"]["text"]
            .as_str()
            .unwrap()
            .contains("/etc/domovoi/helper.toml"),
        "{restart}"
    );

    connect(&a, helper_words(&["unit.restart"]));
    let reply = a.get("/api/v1/hosts/nas/ceiling", &login).await;
    let entries = reply.body["entries"].as_array().expect("entries");
    let restart = entries
        .iter()
        .find(|e| e["contract"] == "unit.restart")
        .unwrap();
    assert_eq!(restart["helper"]["state"], "offered");
}
