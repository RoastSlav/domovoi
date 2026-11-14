//! Installing a backup: the previous database is kept, and a failure puts it back.

use domovoi_core::ports::SettingsRepo;
use domovoi_store::{MIGRATOR, Store, StoreOptions, restore_from_backup};
use domovoi_testkit::build::{T0, at};

/// A database in `dir/hub.db`, with `ui.theme` set to `theme`, and the store closed.
async fn database_with(dir: &std::path::Path, theme: &str) -> std::path::PathBuf {
    let path = dir.join("hub.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    store
        .set("ui.theme", &serde_json::json!(theme))
        .await
        .unwrap();
    store.close().await;
    path
}

async fn theme_of(path: &std::path::Path) -> Option<serde_json::Value> {
    let store = Store::open(path, StoreOptions::beside(path, T0))
        .await
        .unwrap();
    let theme = store.get("ui.theme").await.unwrap();
    store.close().await;
    theme
}

#[tokio::test]
async fn restore_replaces_the_database_and_keeps_the_previous_one() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("backup-theme").await;
    let live = database_with(dir.path(), "live-theme").await;

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();

    let previous = report.previous.expect("the old database is kept");
    assert!(previous.exists(), "{} is gone", previous.display());
    assert!(
        previous
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".before-restore-"),
        "{}",
        previous.display()
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("backup-theme"))
    );
    assert_eq!(
        theme_of(&previous).await,
        Some(serde_json::json!("live-theme"))
    );
    assert_eq!(report.migrations_applied_after, 0);
}

#[tokio::test]
async fn restore_into_an_empty_directory_creates_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = dir.path().join("empty").join("hub.db");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    assert!(report.previous.is_none());
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("from-backup"))
    );
}

#[tokio::test]
async fn restore_refuses_a_corrupt_or_foreign_backup_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let before = std::fs::read(&live).unwrap();

    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"not a database").unwrap();
    assert!(restore_from_backup(&live, &junk, at(60)).await.is_err());

    let foreign = dir.path().join("foreign.db");
    std::fs::write(&foreign, b"SQLite format 3\0").unwrap();
    assert!(restore_from_backup(&live, &foreign, at(60)).await.is_err());

    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the database is untouched"
    );
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));
    assert!(
        !std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .any(|name| name.contains("before-restore")),
        "nothing was moved"
    );
}

#[tokio::test]
async fn restore_refuses_the_database_itself() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    assert!(matches!(
        restore_from_backup(&live, &live, at(60)).await,
        Err(domovoi_store::RestoreError::SameFile)
    ));
}

#[tokio::test]
async fn restore_migrates_an_older_backup_forward() {
    let dir = tempfile::tempdir().unwrap();
    let older_dir = dir.path().join("older");
    std::fs::create_dir_all(&older_dir).unwrap();
    let older = older_dir.join("hub.db");
    let connect = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&older)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect)
        .await
        .unwrap();
    MIGRATOR.run_to(5, &pool).await.unwrap();
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('ui.theme', '\"old\"')")
        .execute(&pool)
        .await
        .ok();
    pool.close().await;
    let backup = dir.path().join("older.db");
    std::fs::copy(&older, &backup).unwrap();
    let live = dir.path().join("live").join("hub.db");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();

    let report = restore_from_backup(&live, &backup, at(60)).await.unwrap();
    assert_eq!(
        report.migrations_applied_after,
        MIGRATOR.iter().count() - 5,
        "the restore migrates the backup forward"
    );
    assert!(
        live.parent().unwrap().join("backups").exists(),
        "a pre-migration copy was taken"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn restore_sets_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = dir.path().join("live").join("hub.db");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();
    restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    let mode = std::fs::metadata(&live).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// A backup file to restore from, built in its own directory.
struct Fresh {
    dir: std::path::PathBuf,
}

impl Fresh {
    fn in_dir(dir: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    async fn write(&self, theme: &str) -> std::path::PathBuf {
        let path = self.dir.join("backup.db");
        let store = Store::open(&path, StoreOptions::beside(&path, T0))
            .await
            .unwrap();
        store
            .set("ui.theme", &serde_json::json!(theme))
            .await
            .unwrap();
        store.close().await;
        path
    }
}

#[tokio::test]
async fn restore_puts_the_old_database_back_when_the_open_fails() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;

    // A backup that verifies as a Domovoi database but cannot be migrated: a table with a
    // name the next migration creates, so opening it fails.
    let broken_dir = dir.path().join("broken");
    std::fs::create_dir_all(&broken_dir).unwrap();
    let broken = broken_dir.join("backup.db");
    let connect = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&broken)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect)
        .await
        .unwrap();
    MIGRATOR.run_to(5, &pool).await.unwrap();
    sqlx::query("CREATE TABLE journal (seq INTEGER PRIMARY KEY, wrong TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let err = restore_from_backup(&live, &broken, at(60))
        .await
        .unwrap_err();
    assert!(
        matches!(err, domovoi_store::RestoreError::Open(_)),
        "the open fails, not the copy: {err}"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("live-theme")),
        "the previous database is back in place"
    );
    // The copy that would not open stays, under `.failed-restore-` on Unix and at its
    // temporary name where the failed open still holds it (Windows).
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .any(|name| { name.contains(".failed-restore-") || name.contains(".restore-tmp") }),
        "the failed copy is kept: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains(".before-restore-")),
        "the previous copy never moved: {names:?}"
    );
}
