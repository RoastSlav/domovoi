//! The rename of `digest_facts` to `metric_samples` and the compatibility view.

use std::path::{Path, PathBuf};

use domovoi_core::DigestFact;
use domovoi_core::ports::MetricSampleRepo;
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::at;
use sqlx::ConnectOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("n1-schema5.db")
}

fn copy_fixture(dir: &Path) -> PathBuf {
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).unwrap();
    let path = dbdir.join("domovoi.db");
    std::fs::copy(fixture(), &path).unwrap();
    path
}

async fn raw(path: &Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .unwrap()
}

const HOST: &str = "0190a0a0-0000-7000-8000-0000000000aa";

/// Writes three samples with explicit ids into the old table, before the migration runs.
async fn seeded(dir: &Path) -> PathBuf {
    let path = copy_fixture(dir);
    let mut conn = raw(&path).await;
    for (id, key) in [(7, "a"), (9, "b"), (12, "c")] {
        sqlx::query(
            "INSERT INTO digest_facts (id, host_id, key, label, value, unit, observed_at) \
             VALUES (?, ?, ?, ?, ?, NULL, ?)",
        )
        .bind(id)
        .bind(HOST)
        .bind(key)
        .bind(format!("label {key}"))
        .bind(format!("\"{key}\""))
        .bind(1000 + id)
        .execute(&mut conn)
        .await
        .unwrap();
    }
    path
}

#[tokio::test]
async fn upgrade_copies_digest_facts_into_metric_samples_keeping_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = seeded(dir.path()).await;
    let store = Store::open(&path, StoreOptions::beside(&path, at(5000)))
        .await
        .unwrap();

    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, key, value FROM metric_samples ORDER BY id")
            .fetch_all(store.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (7, "a".into(), "\"a\"".into()),
            (9, "b".into(), "\"b\"".into()),
            (12, "c".into(), "\"c\"".into()),
        ]
    );
    let kind: (String,) =
        sqlx::query_as("SELECT type FROM sqlite_master WHERE name = 'digest_facts'")
            .fetch_one(store.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(kind.0, "view");

    sqlx::query("INSERT INTO metric_samples (host_id, key, label, value, observed_at) VALUES (?, 'n', 'l', '1', 1)")
        .bind(HOST)
        .execute(store.pool_for_tests())
        .await
        .unwrap();
    let (max,): (i64,) = sqlx::query_as("SELECT max(id) FROM metric_samples")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert!(
        max > 12,
        "a new sample gets an id above the copied ones: {max}"
    );
}

#[tokio::test]
async fn digest_facts_view_serves_the_previous_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = seeded(dir.path()).await;
    let store = Store::open(&path, StoreOptions::beside(&path, at(5000)))
        .await
        .unwrap();
    drop(store);

    let mut conn = raw(&path).await;
    sqlx::query(
        "INSERT INTO digest_facts (host_id, key, label, value, unit, observed_at) \
         VALUES (?, 'old', 'Old', '\"x\"', NULL, 1500)",
    )
    .bind(HOST)
    .execute(&mut conn)
    .await
    .unwrap();
    let in_new: (i64,) = sqlx::query_as("SELECT count(*) FROM metric_samples WHERE key = 'old'")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(in_new.0, 1, "the insert lands in metric_samples");

    let rows: Vec<(String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT key, label, value, unit, observed_at FROM digest_facts \
         WHERE host_id = ? AND observed_at >= ? AND observed_at < ? ORDER BY observed_at, id",
    )
    .bind(HOST)
    .bind(0)
    .bind(2000)
    .fetch_all(&mut conn)
    .await
    .unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(keys, vec!["a", "b", "c", "old"]);
}

#[tokio::test]
async fn knowledge_migration_leaves_existing_tables_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let tables = ["incidents", "notes", "audit_log", "actions"];
    let mut before = Vec::new();
    {
        let mut conn = raw(&path).await;
        for table in tables {
            let (n,): (i64,) =
                sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&mut conn)
                    .await
                    .unwrap();
            before.push(n);
        }
    }
    let store = Store::open(&path, StoreOptions::beside(&path, at(5000)))
        .await
        .unwrap();
    for (table, was) in tables.into_iter().zip(before) {
        let (n,): (i64,) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(store.pool_for_tests())
                .await
                .unwrap();
        assert!(n >= was, "{table}: {n} < {was}");
    }
    let (check,): (String,) = sqlx::query_as("PRAGMA quick_check")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(check, "ok");
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
