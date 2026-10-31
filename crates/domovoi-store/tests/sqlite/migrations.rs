//! Opening runs the embedded migrations once, and backs up a database before migrating it.

use domovoi_core::ports::SettingsRepo;
use domovoi_store::{MIGRATOR, Store, StoreOptions, needs_backup, pre_migrate_name};
use domovoi_testkit::build::{T0, at};
use sqlx::ConnectOptions;

use crate::common::Fresh;

async fn applied(store: &Store) -> Vec<i64> {
    sqlx::query_as::<_, (i64,)>("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap()
        .into_iter()
        .map(|(v,)| v)
        .collect()
}

#[tokio::test]
async fn migrations_run_on_empty_database_and_are_idempotent() {
    let fresh = Fresh::new().await;
    let versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(applied(&fresh.store).await, versions);
    fresh
        .store
        .set("ui.theme", &serde_json::json!("dark"))
        .await
        .unwrap();
    fresh.store.close().await;

    // Opening again applies nothing new, takes no backup and keeps the data.
    let path = fresh.path();
    let options = StoreOptions::beside(&path, at(60));
    let backups = options.backup_dir.clone();
    let reopened = Store::open(&path, options).await.unwrap();
    assert_eq!(applied(&reopened).await, versions);
    assert_eq!(
        reopened.get("ui.theme").await,
        Ok(Some(serde_json::json!("dark")))
    );
    assert!(!backups.exists(), "no pending migrations, no backup");
    reopened.close().await;
}

#[tokio::test]
async fn backup_is_taken_before_pending_migrations() {
    assert!(needs_backup(&[], &[1]));
    assert!(needs_backup(&[1], &[1, 2]));
    assert!(!needs_backup(&[1, 2], &[1, 2]));
    assert!(!needs_backup(&[1], &[]));

    // A database that exists but has never been migrated: its data is backed up first.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("domovoi.db");
    let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    sqlx::query("CREATE TABLE legacy (x TEXT); INSERT INTO legacy VALUES ('kept')")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let options = StoreOptions::beside(&path, T0);
    let backup = options.backup_dir.join(pre_migrate_name(T0));
    let store = Store::open(&path, options).await.unwrap();
    assert!(backup.exists(), "{} was not written", backup.display());
    store.close().await;
    let mut copy = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&backup)
        .connect()
        .await
        .unwrap();
    let (kept,): (String,) = sqlx::query_as("SELECT x FROM legacy")
        .fetch_one(&mut copy)
        .await
        .unwrap();
    assert_eq!(kept, "kept");
    let migrated: Result<(i64,), _> = sqlx::query_as("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&mut copy)
        .await;
    assert!(
        migrated.is_err(),
        "the backup is the database before migrating"
    );

    // A brand-new database has nothing to back up.
    let fresh = Fresh::new().await;
    assert!(!fresh.dir.path().join("db").join("backups").exists());
}

#[tokio::test]
async fn backups_keep_the_newest_copies() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let mut written = Vec::new();
    for day in 0..9 {
        written.push(fresh.store.backup_to(&dir, at(day * 86_400)).await.unwrap());
    }
    let mut left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    left.sort();
    assert_eq!(left, written[2..], "the default keeps 7");
    // The same second twice cannot overwrite a backup.
    assert!(fresh.store.backup_to(&dir, at(8 * 86_400)).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn database_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let fresh = Fresh::new().await;
    let mode = std::fs::metadata(fresh.dir.path().join("db"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}
