//! The release check endpoint and the commands it hands out.

use axum::http::StatusCode;
use domovoi_core::ports::SettingsRepo;
use jiff::SignedDuration;
use serde_json::json;

use crate::api_harness::api;

#[tokio::test]
async fn update_endpoint_returns_commands_for_the_stored_release() {
    let a = api().await;
    let login = a.admin().await;
    SettingsRepo::set(
        &*a.store,
        "update_check",
        &json!({
            "current": "0.0.1",
            "latest": "v0.9.9",
            "newer": true,
            "url": "https://github.com/RoastSlav/domovoi/releases/tag/v0.9.9",
            "notes": "Fixed things",
            "published_at": "2026-11-09T10:00:00Z",
            "checked_at": "2026-11-10T12:00:00Z"
        }),
    )
    .await
    .unwrap();
    a.state.deps().update.reload().await;

    let reply = a.get("/api/v1/system/update", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["latest"], "v0.9.9");
    assert_eq!(reply.body["newer"], true);
    assert_eq!(reply.body["checked_at"], "2026-11-10T12:00:00Z");
    assert!(
        reply.body["hub_command"]
            .as_str()
            .unwrap()
            .contains("--role hub --version v0.9.9")
    );
    // Offered unless the test itself runs in a container.
    if !std::path::Path::new("/.dockerenv").exists()
        && !std::path::Path::new("/run/.containerenv").exists()
    {
        assert_eq!(
            reply.body["upgrade_command"],
            "sudo domovoi upgrade --version v0.9.9 && sudo systemctl restart domovoi-hub"
        );
    }
    assert!(
        reply.body["agent_command"]
            .as_str()
            .unwrap()
            .contains("--role agent --version v0.9.9")
    );
    assert!(
        reply.body["docker_command"]
            .as_str()
            .unwrap()
            .contains("docker compose -f hub.yml pull")
    );
    assert!(
        reply.body["docker_command"]
            .as_str()
            .unwrap()
            .contains("DOMOVOI_IMAGE_TAG=0.9.9")
    );
}

/// A tag that is not a plain release tag is never shown and never reaches a command, so
/// nothing unsafe can be pasted into a shell even if an older build saved it.
#[tokio::test]
async fn a_stored_release_with_an_unsafe_tag_offers_nothing() {
    let a = api().await;
    let login = a.admin().await;
    SettingsRepo::set(
        &*a.store,
        "update_check",
        &json!({
            "current": "0.0.1",
            "latest": "v0.9.9-$(id)",
            "newer": true,
            "url": "https://github.com/RoastSlav/domovoi/releases/tag/v0.9.9-$(id)",
            "notes": "Whatever",
            "published_at": "2026-11-09T10:00:00Z",
            "checked_at": "2026-11-10T12:00:00Z"
        }),
    )
    .await
    .unwrap();
    a.state.deps().update.reload().await;

    let reply = a.get("/api/v1/system/update", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert!(
        reply.body["latest"].is_null(),
        "the unsafe tag is not shown"
    );
    assert_eq!(reply.body["newer"], false, "nothing is claimed newer");
    for command in [
        "hub_command",
        "agent_command",
        "docker_command",
        "upgrade_command",
    ] {
        assert!(
            reply.body[command].as_str().is_none(),
            "no command is offered for it"
        );
    }
}

/// Pressing the button twice in a row is refused; a minute later it is allowed again.
/// The harness has no HTTP client, so a check that is allowed answers 503, and a check
/// that is refused answers 429.
#[tokio::test]
async fn manual_check_is_limited_to_once_a_minute() {
    let a = api().await;
    let login = a.admin().await;

    let first = a
        .post("/api/v1/system/update/check", &login, json!({}))
        .await;
    assert_ne!(
        first.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        first.text
    );

    let again = a
        .post("/api/v1/system/update/check", &login, json!({}))
        .await;
    assert_eq!(
        again.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        again.text
    );

    a.advance(SignedDuration::from_secs(60));
    let later = a
        .post("/api/v1/system/update/check", &login, json!({}))
        .await;
    assert_ne!(
        later.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        later.text
    );
}
