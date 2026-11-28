//! Settings managed from the web UI: saving, validation, locks, passwords, audit,
//! encryption at rest, connection tests and per-host check overrides.

use axum::http::{Method, StatusCode};
use domovoi_core::ports::{AuditFilter, AuditLog, Page, SettingsRepo};
use domovoi_hub::settings::SecretKey;
use domovoi_hub::settings::stored::StoredState;
use serde_json::{Value, json};

use crate::api_harness::{Api, LOCKING_VAR, Login, PASSWORD, RETENTION_LOCKING_VAR, Reply, api};

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
        Some(PASSWORD),
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
async fn llm_cli_change_requires_step_up() {
    let a = api().await;
    let login = a.admin().await;
    let changes = [
        json!({ "llm": { "secondary": { "cli": { "command": "/usr/local/bin/other" } } } }),
        json!({ "llm": { "secondary": { "cli": { "args": ["--dangerously-skip-permissions"] } } } }),
        json!({ "llm": { "secondary": { "cli": { "preset": "codex" } } } }),
        // The whole CLI table at once, which the harness leaves absent: the leaf is the
        // table itself, so the path rule still applies.
        json!({ "llm": { "secondary": { "cli": { "preset": "claude", "args": ["-p"] } } } }),
    ];
    for change in &changes {
        let missing = patch(&a, &login, 0, change.clone(), None).await;
        assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
        assert!(
            missing.body["message"]
                .as_str()
                .unwrap()
                .contains("password"),
            "{}",
            missing.text
        );
        assert_eq!(
            SettingsRepo::get(&*a.store, "config").await.unwrap(),
            None,
            "a refused change stores nothing"
        );
    }
    // The API agrees the primary slot's CLI settings are sensitive too.
    let a2 = api().await;
    let login2 = a2.admin().await;
    let primary = json!({ "llm": { "primary": { "cli": {
        "preset": "claude",
        "command": "/usr/local/bin/claude"
    } } } });
    assert_eq!(
        patch(&a2, &login2, 0, primary, None).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn metrics_token_is_a_secret_and_needs_step_up() {
    let a = api().await;
    let login = a.admin().await;
    let change = json!({ "metrics": { "token": { "set": "scrape-secret-value" } } });
    let missing = patch(&a, &login, 0, change.clone(), None).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
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
    let detail = events[0].detail.to_string();
    assert!(!detail.contains("scrape-secret-value"), "{detail}");
    assert!(detail.contains("metrics.token"), "{detail}");
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
    let body = json!({
        "changes": { "heartbeat": { "url": { "set": secret_url } } },
        "password": PASSWORD,
    });
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
async fn testing_a_candidate_with_a_secret_needs_the_password_and_sends_nothing_without_it() {
    let a = api().await;
    let login = a.admin().await;
    let probe = wiremock::MockServer::start().await;
    let url = format!("{}/internal-probe", probe.uri());
    let candidate = |password: Option<&str>| {
        let mut body = json!({ "changes": { "heartbeat": { "url": { "set": url } } } });
        if let Some(password) = password {
            body["password"] = json!(password);
        }
        body
    };
    let discord = json!({ "changes": { "discord": {
        "enabled": true, "channel_id": 42, "bot_token": { "set": DISCORD_SECRET }
    } } });
    for (path, body) in [
        ("/api/v1/settings/test/heartbeat", candidate(None)),
        ("/api/v1/settings/test/heartbeat", candidate(Some("wrong"))),
        ("/api/v1/settings/test/discord", discord),
    ] {
        let reply = a.post(path, &login, body).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{path}: {}",
            reply.text
        );
        assert!(reply.text.contains("password"), "{path}: {}", reply.text);
    }
    assert!(
        probe.received_requests().await.unwrap().is_empty(),
        "the hub must not have requested the URL without the password"
    );
}

#[tokio::test]
async fn discord_connection_test_passes_validation_before_approvers_are_saved() {
    let a = api().await;
    let login = a.admin().await;
    let body = json!({ "changes": { "discord": {
        "enabled": true,
        "channel_id": 42,
        "bot_token": { "set": DISCORD_SECRET }
    } }, "password": PASSWORD });
    let reply = a
        .post("/api/v1/settings/test/discord", &login, body.clone())
        .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "enabled Discord with no approvers must run, not be rejected: {}",
        reply.text,
    );
    assert_eq!(
        reply.body["ok"], false,
        "the test token cannot reach Discord"
    );
    assert!(
        !reply.text.contains("must list at least one Discord"),
        "the endpoint still rejects a test without approvers: {}",
        reply.text,
    );
    assert!(
        reply.text.contains("Discord refused"),
        "the test reached Discord: {}",
        reply.text,
    );
    assert!(
        a.store.list_prefix("").await.unwrap().is_empty(),
        "nothing stored"
    );
}

#[tokio::test]
async fn model_discovery_uses_candidate_settings_before_a_model_is_chosen() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/v1/models"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "local-model"}]
        })))
        .mount(&server)
        .await;
    let a = api().await;
    let login = a.admin().await;
    let body = json!({ "changes": { "llm": { "primary": {
        "kind": "openai_compatible",
        "base_url": format!("{}/v1", server.uri())
    }}}});

    let reply = a.post("/api/v1/settings/models", &login, body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body, json!({ "models": ["local-model"] }));
    assert!(a.store.list_prefix("").await.unwrap().is_empty());
}

#[tokio::test]
async fn testing_a_cli_candidate_needs_the_password_because_it_runs_the_program() {
    let a = api().await;
    let login = a.admin().await;
    let candidate = |password: Option<&str>| {
        let mut body = json!({ "slot": "primary", "changes": { "llm": { "primary": {
            "kind": "cli",
            "model": "x",
            "cli": { "preset": "claude", "command": "/nonexistent/domovoi-qa-cli" }
        }}}});
        if let Some(password) = password {
            body["password"] = json!(password);
        }
        body
    };

    for path in ["/api/v1/settings/test/llm", "/api/v1/settings/models"] {
        let missing = a.post(path, &login, candidate(None)).await;
        assert_eq!(
            missing.status,
            StatusCode::FORBIDDEN,
            "{path}: {}",
            missing.text
        );
        let wrong = a
            .post(path, &login, candidate(Some("not the password")))
            .await;
        assert_eq!(
            wrong.status,
            StatusCode::FORBIDDEN,
            "{path}: {}",
            wrong.text
        );
    }

    // With the password the program is tried (and fails: it does not exist).
    let tried = a
        .post(
            "/api/v1/settings/test/llm",
            &login,
            candidate(Some(PASSWORD)),
        )
        .await;
    assert_eq!(tried.status, StatusCode::OK, "{}", tried.text);
    assert_eq!(tried.body["ok"], false, "{}", tried.text);

    // An API provider needs no password to be tested.
    let api_provider = json!({ "changes": { "llm": { "primary": {
        "kind": "openai_compatible", "base_url": "http://127.0.0.1:9/v1", "model": "m"
    }}}});
    let reply = a
        .post("/api/v1/settings/test/llm", &login, api_provider)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
}

#[tokio::test]
async fn testing_the_secondary_slot_without_a_secondary_says_no_fallback_is_set_up() {
    let a = api().await;
    let login = a.admin().await;
    let body = json!({ "changes": {}, "slot": "secondary" });
    let reply = a.post("/api/v1/settings/test/llm", &login, body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["ok"], false, "there is nothing to test");
    assert!(
        reply.text.contains("no fallback model is set up"),
        "the answer should name the missing fallback: {}",
        reply.text,
    );
    assert!(
        a.store.list_prefix("").await.unwrap().is_empty(),
        "nothing stored"
    );
}

#[tokio::test]
async fn host_check_overrides_are_stored_and_reported_offline() {
    let a = api().await;
    let login = a.admin().await;
    let body = json!({ "overrides": { "disk.free": { "interval_seconds": 120, "thresholds": { "warning_free_pct": 70 } } } });
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
    assert_eq!(checks.body["checks"][0]["key"], "disk.free");
    assert_eq!(
        checks.body["checks"][0]["overrides"]["interval_seconds"],
        120
    );
    let bad = json!({ "overrides": { "disk.free": { "interval_seconds": 0 } } });
    let reply = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/checks",
            Some(&login),
            Some(bad),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    for (bad, needle) in [
        (json!({ "interval_seconds": 1 }), "interval_seconds"),
        (json!({ "thresholds": { "bogus_key": 1 } }), "bogus_key"),
        (
            json!({ "thresholds": { "warning_free_pct": 500 } }),
            "0..=100",
        ),
    ] {
        let reply = a
            .send(
                Method::PUT,
                "/api/v1/hosts/nas/checks",
                Some(&login),
                Some(json!({ "overrides": { "disk.free": bad } })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
        assert!(reply.text.contains(needle), "{}", reply.text);
    }
}

#[tokio::test]
async fn host_docker_choice_is_stored_and_reported_offline() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/docker",
            Some(&login),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["delivered"], false, "the agent is offline");
    let state = a.get("/api/v1/hosts/nas/docker", &login).await;
    assert_eq!(state.body["connected"], false);
    assert_eq!(state.body["reported"], false);
    assert_eq!(state.body["choice"], false);
    assert_eq!(state.body["enabled"], false);

    let back = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/docker",
            Some(&login),
            Some(json!({ "enabled": null })),
        )
        .await;
    assert_eq!(back.status, StatusCode::OK, "{}", back.text);
    let state = a.get("/api/v1/hosts/nas/docker", &login).await;
    assert_eq!(
        state.body["choice"],
        Value::Null,
        "back to the agent's decision"
    );
}

#[tokio::test]
async fn retention_source_labels_are_reported() {
    let a = api().await;
    let login = a.admin().await;
    let before = editable(&a, &login).await;
    assert_eq!(
        before["fields"]["retention.signals_days"],
        format!("env:{RETENTION_LOCKING_VAR}")
    );
    assert_eq!(before["fields"]["retention.incident_timeline_days"], "file");
    assert!(
        before["fields"]
            .get("retention.conversations_days")
            .is_none(),
        "a default field is absent from the map"
    );

    let saved = patch(
        &a,
        &login,
        0,
        json!({ "retention": { "conversations_days": 200 } }),
        Some(PASSWORD),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    let after = editable(&a, &login).await;
    assert_eq!(after["fields"]["retention.conversations_days"], "ui");

    let file = patch(
        &a,
        &login,
        1,
        json!({ "retention": { "incident_timeline_days": 30 } }),
        Some(PASSWORD),
    )
    .await;
    assert_eq!(file.status, StatusCode::CONFLICT, "{}", file.text);
    assert!(
        file.body["message"]
            .as_str()
            .unwrap()
            .contains("retention.incident_timeline_days is set by file"),
        "{}",
        file.text
    );
    let env = patch(
        &a,
        &login,
        1,
        json!({ "retention": { "signals_days": 30 } }),
        Some(PASSWORD),
    )
    .await;
    assert_eq!(env.status, StatusCode::CONFLICT, "{}", env.text);
    assert!(
        env.body["message"]
            .as_str()
            .unwrap()
            .contains(RETENTION_LOCKING_VAR),
        "{}",
        env.text
    );
}

#[tokio::test]
async fn retention_change_requires_step_up() {
    let a = api().await;
    let login = a.admin().await;
    let change = json!({ "retention": { "triage_cache_days": 60 } });
    let missing = patch(&a, &login, 0, change.clone(), None).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    assert!(
        missing.body["message"]
            .as_str()
            .unwrap()
            .contains("password"),
        "{}",
        missing.text
    );
    assert_eq!(SettingsRepo::get(&*a.store, "config").await.unwrap(), None);

    let ok = patch(&a, &login, 0, change, Some(PASSWORD)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let after = editable(&a, &login).await;
    assert_eq!(after["values"]["retention"]["triage_cache_days"], 60);
    assert_eq!(after["fields"]["retention.triage_cache_days"], "ui");
}

#[tokio::test]
async fn backup_counts_require_step_up() {
    let a = api().await;
    let login = a.admin().await;
    for (n, key) in ["keep", "keep_pre_migrate"].into_iter().enumerate() {
        let change = json!({ "backup": { key: 9 } });
        let missing = patch(&a, &login, n as u64, change.clone(), None).await;
        assert_eq!(
            missing.status,
            StatusCode::FORBIDDEN,
            "{key}: {}",
            missing.text
        );
        assert!(
            missing.body["message"]
                .as_str()
                .unwrap()
                .contains("password")
        );
        let ok = patch(&a, &login, n as u64, change, Some(PASSWORD)).await;
        assert_eq!(ok.status, StatusCode::OK, "{key}: {}", ok.text);
        let after = editable(&a, &login).await;
        assert_eq!(after["values"]["backup"][key], 9);
    }
    let audited = a
        .audit
        .list(&AuditFilter::default(), &Page::first(200))
        .await
        .unwrap();
    assert!(
        audited
            .items
            .iter()
            .filter(|e| e.kind.starts_with("settings"))
            .count()
            >= 2,
        "both changes are audited"
    );
}

#[tokio::test]
async fn declared_retention_keys_are_accepted_but_applied_to_nothing() {
    let a = api().await;
    let login = a.admin().await;
    let reply = patch(
        &a,
        &login,
        0,
        json!({ "retention": { "operation_journal_days": 9 } }),
        Some(PASSWORD),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let after = editable(&a, &login).await;
    assert_eq!(after["values"]["retention"]["operation_journal_days"], 9);
    assert_eq!(after["fields"]["retention.operation_journal_days"], "ui");
}

#[tokio::test]
async fn packages_enabled_change_requires_step_up() {
    let a = api().await;
    let login = a.admin().await;
    let change = json!({ "packages": { "enabled": true, "approval_window": "5m" } });
    let missing = patch(&a, &login, 0, change.clone(), None).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.text);
    assert_eq!(SettingsRepo::get(&*a.store, "config").await.unwrap(), None);

    let ok = patch(&a, &login, 0, change, Some(PASSWORD)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let after = editable(&a, &login).await;
    assert_eq!(after["values"]["packages"]["enabled"], true);
    assert_eq!(after["fields"]["packages.enabled"], "ui");
}

#[tokio::test]
async fn packages_keys_are_not_web_writable() {
    let a = api().await;
    let login = a.admin().await;
    let key = json!({
        "id": "a1b2c3d4e5f60718",
        "public_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    });
    for change in [
        json!({ "packages": { "owner_keys": [key] } }),
        json!({ "packages": { "revoked": ["a1b2c3d4e5f60718"] } }),
    ] {
        // Even with the password: the keys are read from hub.toml only.
        let reply = patch(&a, &login, 0, change, Some(PASSWORD)).await;
        assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
        let message = reply.body["message"].as_str().unwrap();
        assert!(
            message.contains("is set by file") && message.contains("packages."),
            "{message}"
        );
    }
    assert_eq!(SettingsRepo::get(&*a.store, "config").await.unwrap(), None);
}

#[tokio::test]
async fn a_model_test_that_succeeds_clears_the_unreachable_notice() {
    use domovoi_core::ports::AdvisoryRepo;
    let a = api().await;
    let login = a.admin().await;
    let now = jiff::Timestamp::now();
    AdvisoryRepo::upsert(
        &*a.store,
        &domovoi_engine::advisories::triage_degraded("The last 3 triage runs failed", now),
    )
    .await
    .unwrap();
    assert_eq!(AdvisoryRepo::list_open(&*a.store).await.unwrap().len(), 1);

    // A model that does not answer leaves the notice alone.
    let down = json!({ "changes": { "llm": { "primary": {
        "kind": "openai_compatible", "base_url": "http://127.0.0.1:9/v1", "model": "m"
    }}}});
    let reply = a.post("/api/v1/settings/test/llm", &login, down).await;
    assert_eq!(reply.body["ok"], false, "{}", reply.text);
    assert_eq!(AdvisoryRepo::list_open(&*a.store).await.unwrap().len(), 1);

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "c", "object": "chat.completion", "created": 0, "model": "m",
            "choices": [{ "index": 0, "finish_reason": "stop",
                "message": { "role": "assistant", "content": "OK" } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&server)
        .await;
    let up = json!({ "changes": { "llm": { "primary": {
        "kind": "openai_compatible", "base_url": format!("{}/v1", server.uri()), "model": "m"
    }}}});
    let reply = a.post("/api/v1/settings/test/llm", &login, up).await;
    assert_eq!(reply.body["ok"], true, "{}", reply.text);
    assert!(
        AdvisoryRepo::list_open(&*a.store).await.unwrap().is_empty(),
        "the notice is gone after a model answered"
    );
}
