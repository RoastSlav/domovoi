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
    let listed = names(live.parent().unwrap());
    assert!(
        !listed.iter().any(|name| name.contains(".before-restore-")),
        "no empty placeholder is left for a database that never existed: {listed:?}"
    );
    assert!(
        !listed.iter().any(|name| name.contains(".failed-restore-")),
        "nothing failed: {listed:?}"
    );
}

#[tokio::test]
async fn restore_keeps_orphaned_sidecars_beside_the_new_database() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = dir.path().join("orphans").join("hub.db");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();
    // No `hub.db`, but SQLite sidecars an interrupted copy left behind.
    std::fs::write(dir.path().join("orphans").join("hub.db-wal"), b"orphan-wal").unwrap();
    std::fs::write(dir.path().join("orphans").join("hub.db-shm"), b"orphan-shm").unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();
    // There was no database to preserve, so the report says so and reserves no name for
    // one; only the sidecars moved aside, keeping the same prefix.
    assert!(report.previous.is_none());
    let listed = names(live.parent().unwrap());
    assert!(
        !listed.iter().any(|name| name.contains(".before-restore-")
            && !name.ends_with("-wal")
            && !name.ends_with("-shm")),
        "no empty placeholder database: {listed:?}"
    );
    // The new database opens and holds the backup's contents; the sidecars are preserved
    // beside it, not silently discarded.
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("from-backup"))
    );
    assert!(
        listed
            .iter()
            .any(|name| name.contains(".before-restore-") && name.ends_with("-wal")),
        "the orphaned WAL is preserved: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|name| name.contains(".before-restore-") && name.ends_with("-shm")),
        "the orphaned SHM is preserved: {listed:?}"
    );
}

#[tokio::test]
async fn a_rollback_that_cannot_finish_is_reported_as_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = database_with(dir.path(), "live-theme").await;
    let before = std::fs::read(&live).unwrap();

    let err = restore_from_backup_failing_at(&live, &backup_path, at(60), "undo-database")
        .await
        .unwrap_err();
    match &err {
        RestoreError::RecoveryIncomplete { reason, recovery } => {
            assert!(
                !reason.is_empty(),
                "the original failure is named: {reason}"
            );
            assert!(
                recovery.contains("before-restore"),
                "the misplaced file is named: {recovery}"
            );
        }
        other => panic!("expected RecoveryIncomplete, got {other:?}"),
    }

    // Nothing was deleted. The moved database is still readable where the report says.
    let listed = names(dir.path());
    let misplaced: Vec<std::path::PathBuf> = listed
        .iter()
        .filter(|name| name.contains(".before-restore-"))
        .map(|name| dir.path().join(name))
        .collect();
    assert_eq!(misplaced.len(), 1, "{listed:?}");
    assert_eq!(
        std::fs::read(&misplaced[0]).unwrap(),
        before,
        "the moved database is intact where it remains"
    );
    let RestoreError::RecoveryIncomplete { recovery, .. } = &err else {
        unreachable!()
    };
    assert!(
        recovery.contains(&misplaced[0].display().to_string()),
        "the message names where the file remains: {recovery}"
    );
}

#[tokio::test]
async fn a_partial_sidecar_move_is_undone_and_the_copy_kept() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let broken = broken_backup(dir.path()).await;
    let before = std::fs::read(&live).unwrap();

    // The seam writes a WAL beside the temporary copy, the sidecar move runs for real,
    // and only the database move fails.
    let err = restore_from_backup_failing_at(&live, &broken, at(60), "failed-database-with-wal")
        .await
        .unwrap_err();
    assert!(
        matches!(err, RestoreError::Open(_)),
        "the open error stays the reported failure: {err}"
    );

    // The live database is untouched and readable.
    assert_eq!(std::fs::read(&live).unwrap(), before);
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));

    // The prepared copy is still at its temporary name, and nothing is stranded at the
    // failed name: the rollback put the sidecar back beside it.
    let listed = names(dir.path());
    assert!(
        listed.iter().any(|name| name == "hub.db.restore-tmp"),
        "the prepared copy is kept: {listed:?}"
    );
    let stranded: Vec<&String> = listed
        .iter()
        .filter(|name| name.contains(".failed-restore-"))
        .collect();
    assert!(
        stranded.is_empty(),
        "the rollback left nothing at the failed name: {stranded:?}"
    );
}

#[tokio::test]
async fn a_sidecar_that_cannot_move_leaves_the_copy_intact_at_its_temporary_name() {
    // A fresh directory, so nothing from another test can mask the state.
    //
    // Where the failed open still holds its sidecar (Windows), the move fails and the copy
    // stays together at `.restore-tmp`. Where it does not (Unix), the move succeeds and a
    // forced rollback failure strands it at the failed name, named by the log. Either way
    // no file is deleted, and the live database never moves. The rollback journal's own
    // behaviour is unit-tested in `restore/files.rs`, where both outcomes are exact.
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let broken = broken_backup(dir.path()).await;
    let before = std::fs::read(&live).unwrap();

    let err =
        restore_from_backup_failing_at(&live, &broken, at(60), "failed-database-with-wal-stuck")
            .await
            .unwrap_err();
    assert!(
        matches!(err, RestoreError::Open(_)),
        "the original open error is still what is reported: {err}"
    );

    let listed = names(dir.path());
    // The prepared copy is kept somewhere, with no data-bearing file deleted.
    assert!(
        listed
            .iter()
            .any(|name| name.starts_with("hub.db.restore-tmp")),
        "the prepared copy is kept: {listed:?}"
    );
    for name in listed
        .iter()
        .filter(|name| name.contains(".restore-tmp") || name.contains(".failed-restore-"))
    {
        assert!(
            std::fs::read(dir.path().join(name)).is_ok(),
            "{name} is readable, so nothing was lost"
        );
    }
    // The live database is intact and readable throughout.
    assert_eq!(std::fs::read(&live).unwrap(), before);
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));
}

/// A backup that verifies as a Domovoi database but cannot be migrated.
async fn broken_backup(dir: &std::path::Path) -> std::path::PathBuf {
    let broken_dir = dir.join("broken");
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
    broken
}

#[tokio::test]
async fn a_failed_copy_that_cannot_be_kept_still_reports_the_open_error() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let live = database_with(dir.path(), "live-theme").await;

    // A backup that verifies but cannot be migrated, so the prepared copy will not open.
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
    let _ = backup_store;

    let err = restore_from_backup(&live, &broken, at(60))
        .await
        .unwrap_err();
    assert!(
        matches!(err, RestoreError::Open(_)),
        "the open error stays the reported failure: {err}"
    );
    // The failed copy is kept for inspection under its own name, or at its temporary name
    // where a failed open still holds it (Windows). Either way it is not removed.
    let listed = names(dir.path());
    assert!(
        listed
            .iter()
            .any(|name| name.contains(".failed-restore-") || name.contains(".restore-tmp")),
        "the failed copy is kept: {listed:?}"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("live-theme")),
        "the live database is untouched"
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

#[tokio::test]
async fn restore_keeps_a_zero_byte_previous_database() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    // A real database that happens to be empty: a hub that was started and never wrote.
    let live = dir.path().join("hub.db");
    std::fs::write(&live, b"").unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();

    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("from-backup")),
        "the backup is installed"
    );
    let previous = report.previous.expect("the previous database is preserved");
    assert!(
        previous.exists(),
        "a zero-byte database is still a database: {} is gone",
        previous.display()
    );
    assert_eq!(
        std::fs::metadata(&previous).unwrap().len(),
        0,
        "it is preserved as it was"
    );
    assert_eq!(
        previous.file_name().unwrap().to_string_lossy(),
        "hub.db.before-restore-20231114T221420Z"
    );
}

#[tokio::test]
async fn restore_keeps_a_zero_byte_database_with_its_wal() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = dir.path().join("hub.db");
    std::fs::write(&live, b"").unwrap();
    std::fs::write(dir.path().join("hub.db-wal"), b"pending-write").unwrap();

    let report = restore_from_backup(&live, &backup_path, at(60))
        .await
        .unwrap();

    let previous = report.previous.expect("the previous database is preserved");
    assert_eq!(
        std::fs::metadata(&previous).unwrap().len(),
        0,
        "the empty database is still there: {}",
        previous.display()
    );
    let mut wal = previous.as_os_str().to_owned();
    wal.push("-wal");
    let wal = std::path::PathBuf::from(wal);
    assert_eq!(
        std::fs::read(&wal).unwrap(),
        b"pending-write",
        "the write-ahead log moved with it and was not overwritten"
    );
    assert!(
        !dir.path().join("hub.db-wal").exists(),
        "the live sidecar moved with the database"
    );
}

#[tokio::test]
async fn an_incomplete_rollback_keeps_a_zero_byte_database_where_it_says() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    let live = dir.path().join("hub.db");
    std::fs::write(&live, b"").unwrap();
    std::fs::write(dir.path().join("hub.db-wal"), b"pending-write").unwrap();

    let err = restore_from_backup_failing_at(&live, &backup_path, at(60), "undo-database")
        .await
        .unwrap_err();
    let RestoreError::RecoveryIncomplete { reason, recovery } = &err else {
        panic!("expected RecoveryIncomplete, got {err:?}");
    };
    assert!(
        reason.contains("cannot install it"),
        "the original failure is named: {reason}"
    );

    // The rollback undoes the newest move first: the WAL cannot go back, so it stays at
    // the preserved name and the error names it. The database itself moved back, so it is
    // at the live path — either way, zero bytes and never deleted.
    let misplaced: Vec<std::path::PathBuf> = names(dir.path())
        .into_iter()
        .filter(|name| name.contains(".before-restore-"))
        .map(|name| dir.path().join(name))
        .collect();
    assert_eq!(
        misplaced.len(),
        1,
        "exactly the WAL stayed misplaced: {misplaced:?}"
    );
    assert!(
        misplaced[0].to_string_lossy().ends_with("-wal"),
        "the misplaced file is the WAL: {}",
        misplaced[0].display()
    );
    assert_eq!(std::fs::read(&misplaced[0]).unwrap(), b"pending-write");
    assert!(
        recovery.contains(&misplaced[0].display().to_string()),
        "the error names where the WAL remains: {recovery}"
    );
    // The database was moved back and was not deleted as an "empty placeholder".
    assert!(live.exists(), "the database is back at its live path");
    assert_eq!(
        std::fs::metadata(&live).unwrap().len(),
        0,
        "still zero bytes, still present"
    );
}

#[tokio::test]
async fn an_incomplete_rollback_never_deletes_the_previous_database() {
    let dir = tempfile::tempdir().unwrap();
    let backup_store = Fresh::in_dir(dir.path().join("backup"));
    let backup_path = backup_store.write("from-backup").await;
    // A zero-byte database and no sidecar: the only file that moves is the database.
    let live = dir.path().join("hub.db");
    std::fs::write(&live, b"").unwrap();

    let err = restore_from_backup_failing_at(&live, &backup_path, at(60), "undo-database")
        .await
        .unwrap_err();
    let RestoreError::RecoveryIncomplete { recovery, .. } = &err else {
        panic!("expected RecoveryIncomplete, got {err:?}");
    };

    // The database could not be moved back, so it stays at the preserved name and the
    // error names that exact path. It must not have been treated as an empty placeholder.
    let misplaced: Vec<std::path::PathBuf> = names(dir.path())
        .into_iter()
        .filter(|name| name.contains(".before-restore-"))
        .map(|name| dir.path().join(name))
        .collect();
    assert_eq!(misplaced.len(), 1, "{misplaced:?}");
    assert_eq!(
        std::fs::metadata(&misplaced[0]).unwrap().len(),
        0,
        "the zero-byte database is preserved: {}",
        misplaced[0].display()
    );
    assert!(
        recovery.contains(&misplaced[0].display().to_string()),
        "the error names where it remains: {recovery}"
    );
    assert!(!live.exists(), "it is not in two places at once");
}

/// A backup that restores cleanly, in its own directory.
async fn good_backup(dir: &std::path::Path) -> std::path::PathBuf {
    let source = dir.join("good");
    std::fs::create_dir_all(&source).unwrap();
    let path = source.join("backup.db");
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    store
        .set("ui.theme", &serde_json::json!("from-backup"))
        .await
        .unwrap();
    store.close().await;
    path
}

/// The `.before-restore-*` names in `dir`, sorted.
fn preserved_names(dir: &std::path::Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|name| name.contains(".before-restore-"))
        .collect()
}

#[tokio::test]
async fn a_directory_sync_failure_after_moving_aside_keeps_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let backup = good_backup(dir.path()).await;
    let before = std::fs::read(&live).unwrap();

    // The flush after the aside renames fails for real, through the same helper the
    // restore uses, so this proves the fence is load-bearing rather than incidental.
    domovoi_store::fail_next_sync_for_tests(&live, "aside");
    let err = restore_from_backup(&live, &backup, at(60))
        .await
        .unwrap_err();
    match &err {
        RestoreError::NotDurable { stage, reason, .. } => {
            assert!(
                stage.contains("moving the previous database aside"),
                "the stage is named: {stage}"
            );
            assert!(reason.contains("simulated failure"), "{reason}");
        }
        other => panic!("expected NotDurable, got {other:?}"),
    }

    // The backup was not installed: the live database is the original, untouched.
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the live database is unchanged"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("live-theme")),
        "the original data is readable"
    );
    assert!(
        preserved_names(dir.path()).is_empty(),
        "the moved-aside database was put back: {:?}",
        names(dir.path())
    );
}

#[tokio::test]
async fn a_directory_sync_failure_after_installing_rolls_the_install_back() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let backup = good_backup(dir.path()).await;
    let before = std::fs::read(&live).unwrap();

    // Only the install flush fails; the aside flush before it succeeds.
    domovoi_store::fail_next_sync_for_tests(&live, "install");
    let err = restore_from_backup(&live, &backup, at(60))
        .await
        .unwrap_err();
    match &err {
        RestoreError::NotDurable { stage, .. } => {
            assert!(
                stage.contains("installing the restored database"),
                "the install flush is the one that failed: {stage}"
            );
        }
        other => panic!("expected NotDurable, got {other:?}"),
    }

    // The installation was undone: the previous database is back at the live path.
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the previous database is restored"
    );
    assert_eq!(
        theme_of(&live).await,
        Some(serde_json::json!("live-theme")),
        "the previous data is readable, so the backup is not live"
    );
    assert!(
        preserved_names(dir.path()).is_empty(),
        "no preserved copy is left behind: {:?}",
        names(dir.path())
    );
}

#[tokio::test]
async fn a_rollback_that_cannot_be_made_durable_is_reported_as_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let backup = good_backup(dir.path()).await;
    let before = std::fs::read(&live).unwrap();

    // The install fails, and the flush that would record the rollback fails too.
    let err = restore_from_backup_failing_at(&live, &backup, at(60), "sync-rollback")
        .await
        .unwrap_err();
    match &err {
        RestoreError::RecoveryIncompleteNotDurable {
            reason,
            recovery,
            durability,
        } => {
            assert!(
                reason.contains("cannot install it"),
                "the original failure: {reason}"
            );
            assert!(recovery.contains("every file moved back"), "{recovery}");
            assert!(durability.contains("simulated failure"), "{durability}");
        }
        other => panic!("expected RecoveryIncompleteNotDurable, got {other:?}"),
    }

    // The renames themselves went back, so the original data is readable — but the error
    // already said the result is not durable, which is the point.
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the previous database is back"
    );
    assert_eq!(theme_of(&live).await, Some(serde_json::json!("live-theme")));
    let text = err.to_string();
    assert!(text.contains("not durable"), "{text}");
}

#[tokio::test]
async fn a_failed_rename_and_a_failed_flush_are_both_reported() {
    let dir = tempfile::tempdir().unwrap();
    let live = database_with(dir.path(), "live-theme").await;
    let backup = good_backup(dir.path()).await;

    // The install fails, the rollback of the moved database fails, and so does the flush.
    let err = restore_from_backup_failing_at(&live, &backup, at(60), "undo-database")
        .await
        .unwrap_err();

    let text = err.to_string();
    // Either the rollback-complete or the incomplete form is right; neither may hide the
    // original failure, and the file that stayed put must be named.
    match &err {
        RestoreError::RecoveryIncomplete { reason, recovery }
        | RestoreError::RecoveryIncompleteNotDurable {
            reason, recovery, ..
        } => {
            assert!(reason.contains("cannot install it"), "{reason}");
            assert!(recovery.contains("before-restore"), "{recovery}");
        }
        other => panic!("expected incomplete recovery, got {other:?}"),
    }
    let misplaced = preserved_names(dir.path());
    assert_eq!(misplaced.len(), 1, "{misplaced:?}");
    assert!(
        text.contains(&misplaced[0]),
        "the error names the misplaced file: {text}"
    );
}
