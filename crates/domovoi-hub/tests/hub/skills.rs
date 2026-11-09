//! Skill switches from the web UI: a disabled skill stays on the host and in the
//! catalogue, is saved under the host's settings key, and is re-read when the hub
//! builds a new policy, so a restart keeps the switch.

use axum::http::{Method, StatusCode};

use domovoi_core::ports::{AuditFilter, AuditLog, HostRepo, Page, SettingsRepo};
use domovoi_core::{Risk, SkillName, SkillSummary};
use domovoi_hub::agent_server::{SkillPolicy, host_skills_key};
use domovoi_testkit::build::manifest;
use serde_json::json;

use crate::api_harness::{Api, Login, api};

/// The two skills the fake agent announces; one gets switched off.
const HIDDEN: &str = "zpool_status";
const SHOWN: &str = "listening_ports";

/// A name the agent could never have announced: uppercase is not a skill name.
const NOT_A_SKILL: &str = "Restart_Container";

/// A name the agent could have announced but did not: well-formed, absent here.
const UNANNOUNCED: &str = "prune_logs";

/// The skills the fake agent announces for `nas`.
fn announced() -> Vec<SkillSummary> {
    let hidden = manifest(HIDDEN, Risk::Read).unwrap().summary();
    let shown = manifest(SHOWN, Risk::Read).unwrap().summary();
    [hidden, shown].to_vec()
}

fn named(name: &str) -> SkillName {
    SkillName::parse(name).unwrap()
}

/// Whether the hub may run a skill the host lists; `None` when it does not list it.
async fn running(a: &Api, login: &Login, skill: &str) -> Option<bool> {
    let reply = a.get("/api/v1/hosts/nas/skills", login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let listed = reply
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str().unwrap() == skill);
    listed.map(|item| item["enabled"].as_bool().unwrap())
}

/// Turns one `nas` skill on or off and answers with the status the hub gave.
async fn switch(a: &Api, login: &Login, skill: &str, enabled: bool) -> StatusCode {
    let reply = a
        .send(
            Method::PUT,
            &format!("/api/v1/hosts/nas/skills/{skill}"),
            Some(login),
            Some(json!({ "enabled": enabled })),
        )
        .await;
    reply.status
}

/// The policy the hub would have after starting again.
async fn reloaded(a: &Api) -> SkillPolicy {
    let hosts = HostRepo::list_hosts(&*a.store).await.unwrap();
    SkillPolicy::load(a.store.clone(), &hosts).await.unwrap()
}

#[tokio::test]
async fn disabled_skill_stays_listed_and_survives_a_restart() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_skills(a.nas.id, announced());

    assert_eq!(
        running(&a, &login, HIDDEN).await,
        Some(true),
        "the skill starts on"
    );
    assert_eq!(
        running(&a, &login, SHOWN).await,
        Some(true),
        "the other skill starts on"
    );

    assert_eq!(
        switch(&a, &login, HIDDEN, false).await,
        StatusCode::NO_CONTENT,
        "the switch is saved",
    );
    assert_eq!(
        running(&a, &login, HIDDEN).await,
        Some(false),
        "still on the host, now switched off",
    );
    assert_eq!(
        running(&a, &login, SHOWN).await,
        Some(true),
        "the other skill was not touched",
    );
    assert_eq!(
        SettingsRepo::get(&*a.store, &host_skills_key("nas"))
            .await
            .unwrap(),
        Some(json!({ "disabled": [HIDDEN] })),
        "one name under the host's key",
    );

    let policy = reloaded(&a).await;
    assert!(
        policy.is_disabled(a.nas.id, &named(HIDDEN)),
        "a new policy keeps the skill off",
    );
    assert!(
        !policy.is_disabled(a.nas.id, &named(SHOWN)),
        "a new policy leaves the other skill alone",
    );
}

#[tokio::test]
async fn enabling_a_skill_again_leaves_the_host_key_empty() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_skills(a.nas.id, announced());
    assert_eq!(
        switch(&a, &login, HIDDEN, false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        switch(&a, &login, HIDDEN, true).await,
        StatusCode::NO_CONTENT,
        "the switch goes back the same way",
    );
    assert_eq!(
        running(&a, &login, HIDDEN).await,
        Some(true),
        "the skill runs again"
    );
    assert_eq!(
        SettingsRepo::get(&*a.store, &host_skills_key("nas"))
            .await
            .unwrap(),
        Some(json!({ "disabled": [] })),
        "the key is left empty, not dropped",
    );
    assert!(
        !reloaded(&a).await.is_disabled(a.nas.id, &named(HIDDEN)),
        "a new policy runs it again",
    );
}

#[tokio::test]
async fn skill_switches_are_audited_against_the_host_and_skill() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_skills(a.nas.id, announced());
    assert_eq!(
        switch(&a, &login, HIDDEN, false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        switch(&a, &login, HIDDEN, true).await,
        StatusCode::NO_CONTENT
    );

    let events = AuditLog::list(
        &*a.store,
        &AuditFilter {
            subject: Some(format!("host:nas/skill:{HIDDEN}")),
            ..AuditFilter::default()
        },
        &Page::first(10),
    )
    .await
    .unwrap()
    .items;
    assert_eq!(events.len(), 2, "one event per switch");
    assert!(
        events.iter().all(|event| event.actor.starts_with("user:")),
        "a browser session did it",
    );
    let kinds = events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<&str>>();
    assert!(kinds.contains(&"skill.disabled"), "{kinds:?}");
    assert!(kinds.contains(&"skill.enabled"), "{kinds:?}");
    assert_eq!(events[0].detail, json!({ "enabled": true }), "newest first");
}

#[tokio::test]
async fn a_skill_name_the_agent_could_not_have_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_skills(a.nas.id, announced());
    assert_eq!(
        switch(&a, &login, NOT_A_SKILL, false).await,
        StatusCode::BAD_REQUEST,
        "uppercase is not a skill name",
    );
    assert_eq!(
        switch(&a, &login, UNANNOUNCED, false).await,
        StatusCode::NOT_FOUND,
        "a well-formed name the host does not report is not saved",
    );
    assert_eq!(
        running(&a, &login, HIDDEN).await,
        Some(true),
        "the announced skill is still on",
    );
    assert_eq!(
        SettingsRepo::get(&*a.store, &host_skills_key("nas"))
            .await
            .unwrap(),
        None,
        "the refused switch stored nothing",
    );
}
