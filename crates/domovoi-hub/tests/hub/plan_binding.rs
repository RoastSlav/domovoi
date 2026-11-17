//! The API's plan binding labels and hash family.

use axum::http::StatusCode;
use domovoi_core::ActionStatus;
use domovoi_core::Risk;
use domovoi_core::ports::ActionRepo;
use serde_json::json;

use crate::api_harness::api;

#[tokio::test]
async fn approve_api_accepts_the_current_v2_hash() {
    let a = api().await;
    let login = a.admin().await;
    let action = a.propose(Risk::Write).await;
    let shown = action.plan_hash_for_approval().to_string();

    let reply = a
        .post(
            &format!("/api/v1/actions/{}/approve", action.id()),
            &login,
            json!({ "plan_hash": shown }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["plan_hash"], shown, "the v2 hash is the binding");
    assert_eq!(reply.body["plan_binding"], "v2");
    assert_eq!(
        reply.body["plan_hash_v1"],
        action.plan_hash().to_string(),
        "the legacy hash stays available for audit"
    );
}

#[tokio::test]
async fn approve_api_refuses_a_v1_hash_for_a_v2_action() {
    let a = api().await;
    let login = a.admin().await;
    let action = a.propose(Risk::Write).await;

    let reply = a
        .post(
            &format!("/api/v1/actions/{}/approve", action.id()),
            &login,
            json!({ "plan_hash": action.plan_hash().to_string() }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);

    let stored = ActionRepo::get(&*a.store, action.id())
        .await
        .unwrap()
        .expect("the action is still stored");
    assert_eq!(stored.status(), &ActionStatus::Proposed, "nothing changed");
}

#[tokio::test]
async fn action_dto_labels_a_legacy_action_v1_binding() {
    let a = api().await;
    let login = a.admin().await;
    let action = a.propose(Risk::Write).await;
    sqlx::query("UPDATE actions SET plan_hash_v2 = NULL WHERE id = ?")
        .bind(action.id().to_string())
        .execute(a.store.pool_for_tests())
        .await
        .unwrap();

    let reply = a.get("/api/v1/actions?status=proposed", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let item = reply.body["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == action.id().to_string()))
        .expect("the action is listed");
    assert_eq!(item["plan_binding"], "v1");
    assert_eq!(item["plan_hash"], action.plan_hash().to_string());
    assert!(item["plan_hash_v1"].is_string());
}
