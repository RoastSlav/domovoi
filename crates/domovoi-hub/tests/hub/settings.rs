//! Settings managed from the web UI: saving, validation, locks, passwords, audit,
//! encryption at rest, connection tests and per-host check overrides.

use axum::http::{Method, StatusCode};
use domovoi_core::ports::{AuditFilter, AuditLog, Page, SettingsRepo};
use domovoi_hub::settings::SecretKey;
use domovoi_hub::settings::stored::StoredState;
use serde_json::{Value, json};

use crate::api_harness::{Api, LOCKING_VAR, Login, PASSWORD, Reply, api};

async fn patch(a: &Api, login: &Login, base: u64, changes: Value, password: Option<&str>) -> Reply {
    let mut body = json!({ "base_revision": base, "changes": changes });
    if let Some(password) = password {
        body["password"] = json!(password);
    }
    a.send(Method::PATCH, "/api/v1/settings", Some(login), Some(body))
        .await
}

async fn editable(a: &Api, login: &Login) -> Value {
    let reply = a.get("/api/v1/settings", login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body["editable"].clone()
}

const DISCORD_SECRET: &str = "discord-ui-secret-not-for-eyes";

fn discord_change() -> Value {
    json!({ "discord": {
        "enabled": false,
        "channel_id": 42,
        "bot_token": { "set": DISCORD_SECRET }
    }})
}

#[tokio::test]
async fn saving_merges_with_the_other_layers_and_reports_sources() {
    let a = api().await;
    let login = a.admin().await;
    let before = editable(&a, &login).await;
    assert_eq!(before["revision"], 0);
    assert_eq!(before["fields"]["hub.name"], "file");
    assert_eq!(
        before["fields"]["triage.concurrency"],
        format!("env:{LOCKING_VAR}")
    );
    assert!(
        before["fields"].get("notify.digest.time").is_none(),
        "default"
    );

    let reply = patch(
        &a,
        &login,
        0,
        json!({ "notify": { "digest": { "time": "07:30" } } }),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body, json!({ "revision": 1, "applied": false }));
    let after = editable(&a, &login).await;
    assert_eq!(after["revision"], 1);
    assert_eq!(after["values"]["notify"]["digest"]["time"], "07:30");
    assert_eq!(after["fields"]["notify.digest.time"], "ui");
}

#[tokio::test]
async fn stored_secrets_are_encrypted_at_rest() {
    let a = api().await;
    let login = a.admin().await;
    let reply = patch(&a, &login, 0, discord_change(), Some(PASSWORD)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);

    let rows = a.store.list_prefix("").await.unwrap();
    let raw = serde_json::to_string(&rows).unwrap();
    assert!(
        !raw.contains(DISCORD_SECRET),
        "plaintext in the settings table"
    );
    assert!(
        rows.iter()
            .any(|(key, _)| key == "secret.discord.bot_token")
    );
    let view = editable(&a, &login).await;
    assert_eq!(view["values"]["discord"]["bot_token"], "[set]");

    // A ciphertext moved to another field does not open there.
    let sealed = SettingsRepo::get(&*a.store, "secret.discord.bot_token")
        .await
        .unwrap()
        .unwrap();
    SettingsRepo::set(&*a.store, "secret.heartbeat.url", &sealed)
        .await
        .unwrap();
    let key = SecretKey::load_or_create(a.dir.path()).unwrap();
    let state = StoredState::load(&*a.store, &key).await.unwrap();
    assert_eq!(state.unreadable, ["heartbeat.url"]);
    assert!(state.secrets.contains_key("discord.bot_token"));
}

#[tokio::test]
async fn invalid_patch_returns_every_issue_with_its_path_and_stores_nothing() {
    let a = api().await;
    let login = a.admin().await;
    let reply = patch(
        &a,
        &login,
        0,
        json!({
            "incidents": { "confirm_runs": 0 },
            "backup": { "keep": 0 }
        }),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(reply.body["error"], "invalid");
    let paths: Vec<&str> = reply.body["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["severity"] == "error")
        .map(|i| i["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"incidents.confirm_runs"),
        "{paths:?} {}",
        reply.text
    );
    assert!(paths.contains(&"backup.keep"), "{paths:?}");
    assert_eq!(
        SettingsRepo::get(&*a.store, "config").await.unwrap(),
        None,
        "nothing stored"
    );

    let typo = patch(&a, &login, 0, json!({ "notify": { "digset": true } }), None).await;
    assert_eq!(typo.status, StatusCode::BAD_REQUEST);
    assert_eq!(typo.body["issues"][0]["path"], "notify.digset");
}

#[tokio::test]
async fn stale_revision_is_409() {
    let a = api().await;
    let login = a.admin().await;
    let change = json!({ "incidents": { "confirm_runs": 3 } });
    assert_eq!(
        patch(&a, &login, 0, change.clone(), None).await.status,
        StatusCode::OK
    );
    let stale = patch(&a, &login, 0, change, None).await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.text);
    assert!(
        stale.body["message"]
            .as_str()
            .unwrap()
            .contains("changed elsewhere")
    );
}

#[tokio::test]
async fn file_or_env_locked_field_is_409_with_source() {
    let a = api().await;
    let login = a.admin().await;
    let file = patch(&a, &login, 0, json!({ "hub": { "name": "other" } }), None).await;
    assert_eq!(file.status, StatusCode::CONFLICT, "{}", file.text);
    assert!(
        file.body["message"]
            .as_str()
            .unwrap()
            .contains("hub.name is set by file")
    );
    let env = patch(
        &a,
        &login,
        0,
        json!({ "triage": { "concurrency": 1 } }),
        None,
    )
    .await;
    assert_eq!(env.status, StatusCode::CONFLICT);
    assert!(env.body["message"].as_str().unwrap().contains(LOCKING_VAR));
    assert_eq!(SettingsRepo::get(&*a.store, "config").await.unwrap(), None);
}

#[tokio::test]
async fn sensitive_change_requires_password_and_shares_login_rate_limit() {
    let a = api().await;
    let login = a.admin().await;
    let change = json!({ "web": { "trusted_proxies": ["10.0.0.1"] } });
    let missing = patch(&a, &login, 0, change.clone(), None).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    assert!(
        missing.body["message"]
            .as_str()
            .unwrap()
            .contains("password")
    );
    for _ in 0..5 {
        let wrong = patch(&a, &login, 0, change.clone(), Some("not the password")).await;
        assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    }
    let locked = patch(&a, &login, 0, change.clone(), Some(PASSWORD)).await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        locked.text
    );
    assert_eq!(
        a.login(PASSWORD).await.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the same limit guards login"
    );
    a.advance(jiff::SignedDuration::from_mins(2));
    let ok = patch(&a, &login, 0, change, Some(PASSWORD)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let plain = patch(
        &a,
        &login,
        1,
        json!({ "incidents": { "confirm_runs": 3 } }),
        None,
    )
    .await;
    assert_eq!(
        plain.status,
        StatusCode::OK,
        "plain changes need no password"
    );
}

#[tokio::test]
async fn settings_change_writes_audit_with_paths_and_no_secret_values() {
    let a = api().await;
    let login = a.admin().await;
    let mut change = discord_change();
    change["incidents"] = json!({ "confirm_runs": 4 });
    assert_eq!(
        patch(&a, &login, 0, change, Some(PASSWORD)).await.status,
        StatusCode::OK
    );
    let filter = AuditFilter {
        kind: Some("settings.changed".to_owned()),
        ..AuditFilter::default()
    };
    let events = AuditLog::list(&*a.store, &filter, &Page::first(10))
        .await
        .unwrap()
        .items;
    assert_eq!(events.len(), 1);
    assert!(events[0].actor.starts_with("user:"));
    let detail = events[0].detail.to_string();
    assert!(!detail.contains(DISCORD_SECRET), "{detail}");
    let changes = events[0].detail["changes"].as_array().unwrap();
    assert!(changes.contains(&json!({ "path": "discord.bot_token", "secret": "set" })));
    assert!(changes.contains(&json!({ "path": "incidents.confirm_runs", "old": null, "new": 4 })));
}

#[tokio::test]
async fn test_endpoints_persist_nothing_and_are_rate_limited() {
    let a = api().await;
    let login = a.admin().await;
    let secret_url = "http://127.0.0.1:9/ping/test-endpoint-secret";
    let body = json!({ "changes": { "heartbeat": { "url": { "set": secret_url } } } });
    let reply = a
        .post("/api/v1/settings/test/heartbeat", &login, body.clone())
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["ok"], false, "nothing listens on port 9");
    assert!(
        !reply.text.contains("test-endpoint-secret"),
        "{}",
        reply.text
    );
    assert!(
        a.store.list_prefix("").await.unwrap().is_empty(),
        "nothing stored"
    );
    for _ in 1..10 {
        a.post("/api/v1/settings/test/heartbeat", &login, body.clone())
            .await;
    }
    let limited = a
        .post("/api/v1/settings/test/heartbeat", &login, body)
        .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn host_check_overrides_are_stored_and_reported_offline() {
    let a = api().await;
    let login = a.admin().await;
    let body = json!({ "overrides": { "disk.usage": { "interval_seconds": 120, "thresholds": { "warn_percent": 70 } } } });
    let reply = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/checks",
            Some(&login),
            Some(body),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["delivered"], false, "the agent is offline");
    let checks = a.get("/api/v1/hosts/nas/checks", &login).await;
    assert_eq!(checks.body["connected"], false);
    assert_eq!(checks.body["checks"][0]["key"], "disk.usage");
    assert_eq!(
        checks.body["checks"][0]["overrides"]["interval_seconds"],
        120
    );
    let bad = json!({ "overrides": { "disk.usage": { "interval_seconds": 0 } } });
    let reply = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/checks",
            Some(&login),
            Some(bad),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}
