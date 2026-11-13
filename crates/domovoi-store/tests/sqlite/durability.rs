//! Every connection flushes commits, and a healthy database passes the start-up check.

use crate::common::Fresh;

#[tokio::test]
async fn connections_flush_every_commit() {
    let fresh = Fresh::new().await;
    let (synchronous,): (i64,) = sqlx::query_as("PRAGMA synchronous")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(synchronous, 2, "2 is FULL");
}

#[tokio::test]
async fn a_healthy_database_passes_its_integrity_check() {
    let fresh = Fresh::new().await;
    fresh.store.quick_check().await.unwrap();
}
