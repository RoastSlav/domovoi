use axum::http::{Method, StatusCode};
use domovoi_core::ports::{AuditFilter, AuditLog, Page, SettingsRepo};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api};

const URI: &str = "/api/v1/me/preferences";

async fn put(a: &Api, login: &Login, body: Value) -> crate::api_harness::Reply {
    a.send(Method::PUT, URI, Some(login), Some(body)).await
}

async fn audit_count(a: &Api) -> usize {
    AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(200))
        .await
        .unwrap()
        .items
        .len()
}

#[tokio::test]
async fn preferences_require_a_session() {
    let a = api().await;
    let _admin = a.admin().await;
    let read = a.send(Method::GET, URI, None, None).await;
    assert_eq!(read.status, StatusCode::UNAUTHORIZED);
    let write = a
        .send(
            Method::PUT,
            URI,
            None,
            Some(json!({"presentation": {"profile": "standard"}})),
        )
        .await;
    assert_eq!(write.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn put_without_the_csrf_token_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    let no_csrf = Login {
        cookie: login.cookie.clone(),
        csrf: String::new(),
    };
    let refused = put(
        &a,
        &no_csrf,
        json!({"presentation": {"profile": "standard"}}),
    )
    .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let read = a.get(URI, &login).await;
    assert_eq!(read.body["presentation"]["profile"], "detailed");
}

#[tokio::test]
async fn a_new_owner_gets_detailed_and_no_overrides() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get(URI, &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(
        reply.body,
        json!({
            "version": 1,
            "updated_at": null,
            "presentation": {"profile": "detailed", "pages": {}},
        })
    );
}

#[tokio::test]
async fn put_saves_and_get_returns_it() {
    let a = api().await;
    let login = a.admin().await;
    let body =
        json!({"presentation": {"profile": "standard", "pages": {"operation": "essential"}}});
    let saved = put(&a, &login, body).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    assert_eq!(saved.body["version"], 1);
    assert_eq!(saved.body["updated_at"], a.now().to_string());
    assert_eq!(saved.body["presentation"]["profile"], "standard");
    assert_eq!(
        saved.body["presentation"]["pages"]["operation"],
        "essential"
    );
    let read = a.get(URI, &login).await;
    assert_eq!(read.body, saved.body);
}

#[tokio::test]
async fn put_with_a_bad_profile_is_a_400_with_the_hub_text() {
    let a = api().await;
    let login = a.admin().await;
    let reply = put(&a, &login, json!({"presentation": {"profile": "compact"}})).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(
        reply.body,
        json!({
            "error": "bad_request",
            "message": "unknown profile `compact`; use detailed, standard or essential",
        })
    );
}

#[tokio::test]
async fn put_with_a_bad_page_key_is_a_400() {
    let a = api().await;
    let login = a.admin().await;
    let body =
        json!({"presentation": {"profile": "standard", "pages": {"Operation": "essential"}}});
    let reply = put(&a, &login, body).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(reply.body["error"], "bad_request");
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap()
            .starts_with("page key `Operation` must start with a lowercase letter"),
        "{}",
        reply.text
    );
    assert_eq!(a.get(URI, &login).await.body["updated_at"], Value::Null);
}

#[tokio::test]
async fn put_without_presentation_is_a_no_op_that_returns_the_current_value() {
    let a = api().await;
    let login = a.admin().await;
    for body in [json!({}), json!({"presentation": null})] {
        let empty = put(&a, &login, body.clone()).await;
        assert_eq!(empty.status, StatusCode::OK, "{}", empty.text);
        assert_eq!(empty.body["presentation"]["profile"], "detailed");
        assert_eq!(empty.body["updated_at"], Value::Null);
    }
    let saved = put(
        &a,
        &login,
        json!({"presentation": {"profile": "essential"}}),
    )
    .await;
    a.advance(jiff::SignedDuration::from_hours(1));
    for body in [json!({}), json!({"presentation": null})] {
        let same = put(&a, &login, body).await;
        assert_eq!(same.status, StatusCode::OK, "{}", same.text);
        assert_eq!(same.body, saved.body);
    }
}

#[tokio::test]
async fn put_ignores_unknown_body_keys() {
    let a = api().await;
    let login = a.admin().await;
    let body = json!({
        "user": "someone-else",
        "user_id": "00000000-0000-0000-0000-000000000000",
        "presentation": {"profile": "standard", "extra": 1},
    });
    let reply = put(&a, &login, body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["presentation"]["profile"], "standard");
    assert!(reply.body.get("user").is_none());
}

#[tokio::test]
async fn saving_preferences_changes_no_setting_approval_or_audit_row() {
    let a = api().await;
    let login = a.admin().await;
    let settings_before = a.get("/api/v1/settings", &login).await;
    assert_eq!(settings_before.status, StatusCode::OK);
    let audit_before = audit_count(&a).await;
    let rows_before = SettingsRepo::list_prefix(&*a.store, "").await.unwrap();

    let body =
        json!({"presentation": {"profile": "essential", "pages": {"operation": "standard"}}});
    assert_eq!(put(&a, &login, body).await.status, StatusCode::OK);

    let settings_after = a.get("/api/v1/settings", &login).await;
    assert_eq!(settings_after.body, settings_before.body);
    assert_eq!(audit_count(&a).await, audit_before);
    let rows_after = SettingsRepo::list_prefix(&*a.store, "").await.unwrap();
    let added: Vec<_> = rows_after
        .iter()
        .filter(|row| !rows_before.contains(row))
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(added[0].starts_with("preferences."), "{added:?}");
}

#[tokio::test]
async fn the_openapi_document_lists_the_preferences_operations() {
    let doc = serde_json::to_value(domovoi_hub::api::openapi()).unwrap();
    let path = &doc["paths"]["/api/v1/me/preferences"];
    assert_eq!(path["get"]["operationId"], "get_preferences");
    assert_eq!(path["put"]["operationId"], "update_preferences");
    assert_eq!(path["get"]["tags"], json!(["me"]));
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["PresentationDto"]["properties"]["profile"]["type"],
        "string"
    );
    for name in ["PreferencesDto", "PresentationDto", "PreferencesUpdateDto"] {
        assert!(schemas.get(name).is_some(), "{name}");
    }
}
