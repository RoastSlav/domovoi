//! Installing a backup: the previous database is kept, and a failure puts it back.

use domovoi_core::ports::SettingsRepo;
use domovoi_store::{
    MIGRATOR, RestoreError, Store, StoreOptions, pre_migrate_name, restore_from_backup,
    restore_from_backup_failing_at,
};
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

/// Every file in `dir`, by name, sorted.
fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn restore_never_overwrites_an_existing_previous_database() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "first-live").await;

    // Both restores use the same directory and the same supplied timestamp, so the
    // preserved name collides.
    let first = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    let first_previous = first.previous.expect("the first database is kept");
    let first_bytes = std::fs::read(&first_previous).unwrap();

    // A fresh live database in the same place, restored at the same time.
    let store = Store::open(&live, StoreOptions::beside(&live, T0))
        .await
        .unwrap();
    store
        .set("ui.theme", &serde_json::json!("second-live"))
        .await
        .unwrap();
    store.close().await;
    let second = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    let second_previous = second.previous.expect("the second database is kept");

    assert_ne!(
        first_previous, second_previous,
        "the second restore took a different name"
    );
    assert!(
        second_previous
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-2"),
        "the collision got a suffix: {}",
        second_previous.display()
    );
    assert_eq!(
        std::fs::read(&first_previous).unwrap(),
        first_bytes,
        "the first preserved database is byte-for-byte intact"
    );
    assert_eq!(
        theme_of(&first_previous).await,
        Some(serde_json::json!("first-live"))
    );
    assert_eq!(
        theme_of(&second_previous).await,
        Some(serde_json::json!("second-live"))
    );
}

#[tokio::test]
async fn restore_refuses_to_overwrite_an_existing_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "live-theme").await;

    // A previous failed attempt left a temporary copy behind.
    let leftover = dir.path().join("hub.db.restore-tmp");
    std::fs::write(&leftover, b"left by an earlier run").unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&leftover).unwrap(),
        b"left by an earlier run",
        "the leftover is untouched"
    );
    assert!(
        report.previous.as_ref().is_some_and(|path| path.exists()),
        "the restore itself succeeded under another name"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("from-backup"))
    );
}

#[tokio::test]
async fn restore_carries_the_previous_database_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "live-theme").await;
    // Sidecars beside the live database, as a hub that stopped abruptly leaves them.
    std::fs::write(dir.path().join("hub.db-wal"), b"wal").unwrap();
    std::fs::write(dir.path().join("hub.db-shm"), b"shm").unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    let previous = report.previous.expect("the old database is kept");
    let mut wal = previous.as_os_str().to_owned();
    wal.push("-wal");
    let mut shm = previous.as_os_str().to_owned();
    shm.push("-shm");
    assert_eq!(std::fs::read(&wal).unwrap(), b"wal");
    assert_eq!(std::fs::read(&shm).unwrap(), b"shm");
    assert!(
        !dir.path().join("hub.db-wal").exists(),
        "the live sidecars moved with the database"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("from-backup"))
    );
}

#[tokio::test]
async fn a_sidecar_that_cannot_move_aside_leaves_the_database_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "live-theme").await;
    std::fs::write(dir.path().join("hub.db-shm"), b"shm").unwrap();
    let before = std::fs::read(&live).unwrap();

    let err = restore_from_backup_failing_at(&live, &backup_path, at(60), "aside-shm")
        .await
        .unwrap_err();
    assert!(matches!(err, RestoreError::Io { .. }), "{err}");
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the database did not move"
    );
    assert!(
        dir.path().join("hub.db-shm").exists(),
        "the sidecar is back where it was"
    );
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));
}

#[tokio::test]
async fn a_sidecar_that_cannot_be_installed_puts_everything_back() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "live-theme").await;
    let before = std::fs::read(&live).unwrap();

    let err = restore_from_backup_failing_at(&live, &backup_path, at(60), "swap-database")
        .await
        .unwrap_err();
    assert!(matches!(err, RestoreError::Io { .. }), "{err}");
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the previous database is back in place"
    );
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));
    let listed = names(dir.path());
    assert!(
        !listed.iter().any(|name| name.contains(".before-restore-")),
        "the preserved copy is not left behind: {listed:?}"
    );
}

#[tokio::test]
async fn restore_names_its_pre_migration_copy_with_the_supplied_clock() {
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
    pool.close().await;
    let backup = dir.path().join("older.db");
    std::fs::copy(&older, &backup).unwrap();

    let live_dir = dir.path().join("live");
    std::fs::create_dir_all(&live_dir).unwrap();
    let live = live_dir.join("hub.db");
    let now = at(12_345);
    let report = restore_from_backup(&live, &backup, now).await.unwrap();
    assert!(report.migrations_applied_after > 0);
    // The pre-migration copy the open took is named for `now`, not the wall clock.
    assert!(
        live_dir
            .join("backups")
            .join(pre_migrate_name(now))
            .exists(),
        "the pre-migration copy is named for the supplied time: {:?}",
        names(&live_dir.join("backups"))
    );
}
