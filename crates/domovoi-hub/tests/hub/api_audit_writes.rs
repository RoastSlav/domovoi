//! Writes that must leave an audit row: logout, invites, revokes and notes.

use axum::http::{Method, StatusCode};
use serde_json::json;

use crate::api_harness::api;

#[tokio::test]
async fn logout_invite_revoke_and_notes_each_leave_an_audit_row() {
    let a = api().await;
    let login = a.admin().await;

    let invite = a
        .post("/api/v1/hosts/backup-1/enrollment", &login, json!({}))
        .await;
    assert_eq!(invite.status, StatusCode::OK, "{}", invite.text);
    let rows = a.audit_events("host.invite_created").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].subject, "host:backup-1");
    assert!(
        !rows[0].detail.to_string().contains("enroll-token"),
        "the token is never recorded"
    );

    let note = a
        .post(
            "/api/v1/notes",
            &login,
            json!({ "host": "nas", "title": "Backups", "body_markdown": "secret body" }),
        )
        .await;
    let id = note.body["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/v1/notes/{id}");
    a.send(
        Method::PUT,
        &uri,
        Some(&login),
        Some(json!({ "host": "nas", "title": "Backups 2", "body_markdown": "x" })),
    )
    .await;
    a.send(Method::DELETE, &uri, Some(&login), None).await;
    for kind in ["note.created", "note.updated", "note.deleted"] {
        let rows = a.audit_events(kind).await;
        assert_eq!(rows.len(), 1, "{kind}");
        assert_eq!(rows[0].subject, format!("note:{id}"));
        assert!(!rows[0].detail.to_string().contains("secret body"));
    }

    let wrong = a
        .post(
            "/api/v1/hosts/nas/revoke",
            &login,
            json!({ "confirm": "x" }),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    assert!(
        a.audit_events("host.agent_revoked").await.is_empty(),
        "a refused revoke leaves no row"
    );

    let out = a.post("/api/v1/auth/logout", &login, json!({})).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    let rows = a.audit_events("auth.logout").await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].actor.starts_with("user:"));
}
