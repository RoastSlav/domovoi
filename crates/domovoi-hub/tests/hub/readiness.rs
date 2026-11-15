//! `GET /readyz`: readiness separate from liveness, and what it discloses.

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use domovoi_core::ports::{StoreError, StoreHealth};
use serde_json::json;

use crate::api_harness::{Api, Reply, api};

async fn readyz(a: &Api) -> Reply {
    a.send(Method::GET, "/readyz", None, None).await
}

/// A store that answers each readiness question as a test wants it answered.
struct Stub {
    writable: Result<(), StoreError>,
    current: Result<bool, StoreError>,
}

#[async_trait]
impl StoreHealth for Stub {
    async fn writable(&self) -> Result<(), StoreError> {
        self.writable.clone()
    }

    async fn migrations_current(&self) -> Result<bool, StoreError> {
        self.current.clone()
    }
}

#[tokio::test]
async fn readyz_is_ready_when_the_store_and_services_are_up() {
    let a = api().await;
    let reply = readyz(&a).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body, json!({ "ready": true, "failing": [] }));
    assert_eq!(
        a.send(Method::GET, "/healthz", None, None).await.text,
        "ok",
        "liveness is unchanged"
    );
}

#[tokio::test]
async fn readyz_names_each_failing_check_and_nothing_else() {
    let unwritable = api().await;
    let app = unwritable.with_store_health(Arc::new(Stub {
        writable: Err(StoreError::Backend("disk is read-only".to_owned())),
        current: Ok(true),
    }));
    let reply = unwritable
        .send_to(app, Method::GET, "/readyz", None, None)
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["ready"], false);
    assert_eq!(reply.body["failing"], json!(["store_writable"]));
    let text = reply.text.to_lowercase();
    assert!(
        !text.contains("disk"),
        "error text is never returned: {text}"
    );
    assert!(!text.contains("read-only"), "{text}");

    let behind = api().await;
    let app = behind.with_store_health(Arc::new(Stub {
        writable: Ok(()),
        current: Ok(false),
    }));
    let reply = behind
        .send_to(app, Method::GET, "/readyz", None, None)
        .await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["failing"], json!(["migrations"]));

    let unreadable = api().await;
    let app = unreadable.with_store_health(Arc::new(Stub {
        writable: Ok(()),
        current: Err(StoreError::Backend("the file went away".to_owned())),
    }));
    assert_eq!(
        unreadable
            .send_to(app, Method::GET, "/readyz", None, None)
            .await
            .body["failing"],
        json!(["migrations"]),
        "an error counts as failing"
    );
}

#[tokio::test]
async fn readyz_reports_a_stopped_service() {
    let manager_down = api().await;
    manager_down.stop_manager().await;
    let reply = readyz(&manager_down).await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["failing"], json!(["incident_manager"]));

    let actions_down = api().await;
    actions_down.stop_action_service().await;
    let reply = readyz(&actions_down).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["failing"], json!(["action_service"]));
}
