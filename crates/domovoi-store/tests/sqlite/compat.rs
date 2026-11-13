//! Schema compatibility: refusing a newer database and ignoring unknown migrations.

use domovoi_store::{MIN_READER, Store, StoreOpenError, StoreOptions};
use domovoi_testkit::build::at;
use sqlx::ConnectOptions;

use crate::common::Fresh;

async fn raw_connection(path: &std::path::Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .unwrap()
}

#[tokio::test]
async fn database_requiring_a_newer_binary_is_refused_untouched() {
    let fresh = Fresh::new().await;
    fresh.store.close().await;
    let path = fresh.path();
    let mut conn = raw_connection(&path).await;
    sqlx::query("UPDATE schema_compat SET min_reader = '999.0.0' WHERE id = 1")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let before = std::fs::read(&path).unwrap();
    let options = StoreOptions::beside(&path, at(1000));
    let backups = options.backup_dir.clone();
    let refused = Store::open(&path, options).await;
    assert!(matches!(
        refused,
        Err(StoreOpenError::SchemaTooNew { min_reader, .. }) if min_reader == "999.0.0"
    ));
    assert!(!backups.exists(), "no backup before the refusal");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is untouched"
    );
}

#[tokio::test]
async fn older_binary_ignores_migrations_it_does_not_know() {
    let fresh = Fresh::new().await;
    fresh.store.close().await;
    let path = fresh.path();
    let mut conn = raw_connection(&path).await;
    sqlx::query(
        "INSERT INTO _sqlx_migrations \
         (version, description, installed_on, success, checksum, execution_time) \
         VALUES (999999, 'from a newer binary', 0, 1, x'00', 0)",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let reopened = Store::open(&path, StoreOptions::beside(&path, at(60)))
        .await
        .expect("unknown applied migrations are ignored");
    reopened.close().await;
}

#[tokio::test]
async fn written_by_is_kept_when_nothing_migrated() {
    let fresh = Fresh::new().await;
    fresh.store.close().await;
    let path = fresh.path();
    let mut conn = raw_connection(&path).await;
    sqlx::query("UPDATE schema_compat SET written_by = '9.9.9' WHERE id = 1")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let reopened = Store::open(&path, StoreOptions::beside(&path, at(60)))
        .await
        .unwrap();
    let (written_by, min_reader): (String, String) =
        sqlx::query_as("SELECT written_by, min_reader FROM schema_compat WHERE id = 1")
            .fetch_one(reopened.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(written_by, "9.9.9");
    assert_eq!(min_reader, MIN_READER);
    reopened.close().await;
}
