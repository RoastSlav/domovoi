//! Metric samples and the `digest_facts` compatibility view over them.

use domovoi_core::DigestFact;
use domovoi_core::ports::MetricSampleRepo;
use domovoi_testkit::build::at;

const HOST: &str = "0190a0a0-0000-7000-8000-0000000000aa";

#[tokio::test]
async fn digest_facts_view_reads_and_writes_metric_samples() {
    let fresh = crate::common::Fresh::new().await;
    let pool = fresh.store.pool_for_tests();
    for (key, ms) in [("a", 1000), ("b", 1100), ("old", 1500)] {
        sqlx::query(
            "INSERT INTO digest_facts (host_id, key, label, value, unit, observed_at) \
             VALUES (?, ?, 'L', '\"x\"', NULL, ?)",
        )
        .bind(HOST)
        .bind(key)
        .bind(ms)
        .execute(pool)
        .await
        .unwrap();
    }
    let in_new: (i64,) = sqlx::query_as("SELECT count(*) FROM metric_samples")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(in_new.0, 3, "the inserts land in metric_samples");

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT key FROM digest_facts \
         WHERE host_id = ? AND observed_at >= ? AND observed_at < ? ORDER BY observed_at, id",
    )
    .bind(HOST)
    .bind(0)
    .bind(2000)
    .fetch_all(pool)
    .await
    .unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(keys, vec!["a", "b", "old"]);
}

#[tokio::test]
async fn metric_samples_are_recorded_and_listed_through_the_port() {
    let fresh = crate::common::Fresh::new().await;
    let host = domovoi_core::HostId::new();
    let fact = DigestFact {
        host,
        key: "k".into(),
        label: "L".into(),
        unit: None,
        value: domovoi_core::FactValue::Text("v".into()),
        observed_at: at(100),
    };
    fresh.store.record(&fact).await.unwrap();
    let listed = fresh.store.list(host, at(0), at(200)).await.unwrap();
    assert_eq!(listed.len(), 1);
}
