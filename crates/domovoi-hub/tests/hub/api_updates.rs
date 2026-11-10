//! The release check endpoint and the commands it hands out.

use axum::http::StatusCode;
use domovoi_core::ports::SettingsRepo;
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
}
