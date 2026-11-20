//! The API's plan binding labels and hash family.

use axum::http::StatusCode;
use domovoi_core::Risk;

use crate::api_harness::api;

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
