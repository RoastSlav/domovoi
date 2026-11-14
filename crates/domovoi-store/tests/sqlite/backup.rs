//! Backups are verified before older copies go, and pre-migration copies are pruned.

use domovoi_core::ports::AdvisoryRepo;
use domovoi_core::{Advisory, AdvisoryId, AdvisoryKind};
use domovoi_store::{
    BackupError, Depth, MIGRATOR, Store, StoreOpenError, verify_backup, verify_or_tracked_for_tests,
};
use domovoi_testkit::build::{T0, at};
use sqlx::{ConnectOptions, Connection};

use crate::common::Fresh;

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn backup_verification_accepts_a_fresh_backup() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let path = fresh.store.backup_to(&dir, at(0)).await.unwrap();
    let report = verify_backup(&path, Depth::Full).await.unwrap();
    assert_eq!(report.migrations, MIGRATOR.iter().count());
    assert!(report.size_bytes > 0);
    assert_eq!(
        report.newest_migration,
        MIGRATOR.iter().map(|m| m.version).max().unwrap()
    );
    fresh.store.close().await;
}

#[tokio::test]
async fn corrupt_backup_is_rejected_and_removed() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let good = fresh.store.backup_to(&dir, at(0)).await.unwrap();
    // SQLite checks the pages reachable from the schema: filling them with noise leaves
    // the file a SQLite database that fails its own integrity check.
    let bytes = std::fs::read(&good).unwrap();
    let mut damaged = bytes.clone();
    let page = 4096;
    for slot in 2..damaged.len() / page {
        let start = slot * page;
        for (offset, byte) in damaged[start..start + page].iter_mut().enumerate() {
            *byte = (offset % 251) as u8;
        }
    }
    let path = fresh.dir.path().join("damaged.db");
    std::fs::write(&path, &damaged).unwrap();
    match verify_backup(&path, Depth::Quick).await {
        Err(domovoi_store::BackupError::Corrupt(_))
        | Err(domovoi_store::BackupError::Unreadable(_)) => {}
        other => panic!("a damaged backup was not refused: {other:?}"),
    }
    // A file that is not SQLite at all.
    let junk = fresh.dir.path().join("junk.db");
    std::fs::write(&junk, b"not a database at all").unwrap();
    assert!(verify_backup(&junk, Depth::Quick).await.is_err());
    fresh.store.close().await;
}

#[tokio::test]
async fn failed_backup_does_not_prune_good_ones() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let mut keep = Vec::new();
    for day in 0..3 {
        keep.push(fresh.store.backup_to(&dir, at(day * 86_400)).await.unwrap());
    }
    // `keep_backups` is 7 by default; the three copies stay whatever happens next.
    let options = domovoi_store::StoreOptions {
        backup_dir: dir.clone(),
        keep_backups: 3,
        keep_pre_migrate: 3,
        now: T0,
    };
    let path = fresh.path();
    fresh.store.close().await;
    let reopened = Store::open(&path, options).await.unwrap();
    // The same second twice cannot overwrite a backup: the write fails before any prune.
    assert!(reopened.backup_to(&dir, at(2 * 86_400)).await.is_err());
    for path in &keep {
        assert!(
            path.exists(),
            "{} was pruned by a failed backup",
            path.display()
        );
    }
    reopened.close().await;
}

#[tokio::test]
async fn backup_from_a_newer_build_is_refused() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let good = fresh.store.backup_to(&dir, at(0)).await.unwrap();
    fresh.store.close().await;

    let newer = fresh.dir.path().join("newer.db");
    std::fs::copy(&good, &newer).unwrap();
    let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&newer)
        .connect()
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) \
         VALUES (999999, 'from the future', 0, 1, X'00', 0)",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    let unfinished = fresh.dir.path().join("unfinished.db");
    std::fs::copy(&good, &unfinished).unwrap();
    let mut conn2 = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&unfinished)
        .connect()
        .await
        .unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET success = 0 WHERE version = 1")
        .execute(&mut conn2)
        .await
        .unwrap();

    let foreign = fresh.dir.path().join("foreign.db");
    let mut conn3 = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&foreign)
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    sqlx::query("CREATE TABLE something (x TEXT)")
        .execute(&mut conn3)
        .await
        .unwrap();

    assert!(matches!(
        verify_backup(&newer, Depth::Quick).await.unwrap_err(),
        domovoi_store::BackupError::Newer(999_999)
    ));
    assert!(matches!(
        verify_backup(&unfinished, Depth::Quick).await.unwrap_err(),
        domovoi_store::BackupError::Unfinished(1)
    ));
    assert!(matches!(
        verify_backup(&foreign, Depth::Quick).await.unwrap_err(),
        domovoi_store::BackupError::NotDomovoi
    ));
    conn.close().await.unwrap();
    conn2.close().await.unwrap();
    conn3.close().await.unwrap();
}

#[tokio::test]
async fn pre_migrate_backups_are_pruned_to_the_last_three() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db").join("domovoi.db");
    let backups = path.parent().unwrap().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Five older pre-migration copies, one nightly copy and an unrelated file.
    for day in 0..5 {
        std::fs::write(
            backups.join(domovoi_store::pre_migrate_name(at(day * 86_400))),
            b"old",
        )
        .unwrap();
    }
    std::fs::write(backups.join("domovoi-20000101T000000Z.db"), b"nightly").unwrap();
    std::fs::write(backups.join("notes.txt"), b"notes").unwrap();

    // A database that has never been migrated takes a new copy, which is the newest.
    let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    sqlx::query("CREATE TABLE legacy (x TEXT)")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();

    let options = domovoi_store::StoreOptions::beside(&path, at(9 * 86_400));
    let store = Store::open(&path, options).await.unwrap();
    store.close().await;

    let pre: Vec<String> = names(&backups)
        .into_iter()
        .filter(|name| name.starts_with("pre-migrate-"))
        .collect();
    assert_eq!(pre.len(), 3, "{pre:?}");
    assert!(
        pre.contains(&domovoi_store::pre_migrate_name(at(9 * 86_400))),
        "the new copy is kept"
    );
    assert!(pre.contains(&domovoi_store::pre_migrate_name(at(4 * 86_400))));
    assert!(pre.contains(&domovoi_store::pre_migrate_name(at(3 * 86_400))));
    assert!(backups.join("domovoi-20000101T000000Z.db").exists());
    assert!(backups.join("notes.txt").exists());
}

#[tokio::test]
async fn zero_pre_migrate_keeps_the_newest_one() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    for day in 0..3 {
        std::fs::write(
            backups.join(domovoi_store::pre_migrate_name(at(day * 86_400))),
            b"old",
        )
        .unwrap();
    }
    domovoi_store::prune_pre_migrate_for_tests(&backups, 0).unwrap();
    assert_eq!(
        names(&backups),
        [domovoi_store::pre_migrate_name(at(2 * 86_400))]
    );
}

#[tokio::test]
async fn backup_failed_advisory_round_trips_and_keeps_existing_ones() {
    let fresh = Fresh::new().await;
    let existing = Advisory {
        id: AdvisoryId::new(),
        kind: AdvisoryKind::TriageDegraded,
        host: None,
        title: "Triage is degraded".to_owned(),
        detail: "it is".to_owned(),
        created_at: T0,
        resolved_at: None,
    };
    AdvisoryRepo::upsert(&fresh.store, &existing).await.unwrap();
    let backup = Advisory {
        id: AdvisoryId::new(),
        kind: AdvisoryKind::BackupFailed,
        host: None,
        title: "The nightly backup failed".to_owned(),
        detail: "disk full. The newest earlier backup is still there.".to_owned(),
        created_at: at(60),
        resolved_at: None,
    };
    AdvisoryRepo::upsert(&fresh.store, &backup).await.unwrap();
    let open = fresh.store.list_open().await.unwrap();
    assert!(open.contains(&existing), "the existing advisory survives");
    assert!(open.contains(&backup), "backup_failed round-trips");
    fresh.store.close().await;
}

#[tokio::test]
async fn backup_verification_rejects_a_newer_min_reader() {
    let fresh = Fresh::new().await;
    let dir = fresh.dir.path().join("nightly");
    let good = fresh.store.backup_to(&dir, at(0)).await.unwrap();
    fresh.store.close().await;

    let copied = fresh.dir.path().join("newer-reader.db");
    std::fs::copy(&good, &copied).unwrap();
    let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&copied)
        .connect()
        .await
        .unwrap();
    // A reader requirement no present or past binary can meet.
    sqlx::query("UPDATE schema_compat SET min_reader = '99.0.0' WHERE id = 1")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();

    let err = verify_backup(&copied, Depth::Quick).await.unwrap_err();
    match err {
        BackupError::Incompatible { min_reader, binary } => {
            assert_eq!(min_reader, "99.0.0");
            assert_eq!(binary, env!("CARGO_PKG_VERSION"), "this binary's version");
        }
        other => panic!("expected Incompatible, got {other:?}"),
    }
}

#[tokio::test]
async fn restore_refuses_a_backup_from_a_newer_reader_without_moving_anything() {
    use domovoi_core::ports::SettingsRepo;
    use domovoi_store::restore_from_backup;

    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("hub.db");
    let store = Store::open(&live, domovoi_store::StoreOptions::beside(&live, T0))
        .await
        .unwrap();
    store
        .set("ui.theme", &serde_json::json!("live-theme"))
        .await
        .unwrap();
    store.close().await;
    let before = std::fs::read(&live).unwrap();

    let dir_backup = newer_reader_backup(dir.path()).await;

    let err = restore_from_backup(&live, &dir_backup, at(60))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            domovoi_store::RestoreError::Backup(BackupError::Incompatible { .. })
        ),
        "{err}"
    );
    assert_eq!(std::fs::read(&live).unwrap(), before, "nothing moved");
    assert_eq!(
        names(dir.path())
            .iter()
            .filter(|name| name.contains("before-restore") || name.contains("restore-tmp"))
            .count(),
        0,
        "no artifact was left: {:?}",
        names(dir.path())
    );
}

/// A backup whose `schema_compat.min_reader` no binary can meet.
async fn newer_reader_backup(dir: &std::path::Path) -> std::path::PathBuf {
    let source = dir.join("reader-source");
    std::fs::create_dir_all(&source).unwrap();
    let path = source.join("backup.db");
    let store = Store::open(&path, domovoi_store::StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    store.close().await;
    let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .connect()
        .await
        .unwrap();
    sqlx::query("UPDATE schema_compat SET min_reader = '99.0.0' WHERE id = 1")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    path
}

#[tokio::test]
async fn a_failed_verification_removes_the_new_backup_and_keeps_the_good_ones() {
    let dir = tempfile::tempdir().unwrap();
    let nightly = dir.path().join("nightly");
    std::fs::create_dir_all(&nightly).unwrap();
    // Two good backups that must survive, and an unrelated file.
    for name in ["domovoi-20260101T040000Z.db", "domovoi-20260102T040000Z.db"] {
        std::fs::write(nightly.join(name), b"good").unwrap();
    }
    std::fs::write(nightly.join("notes.txt"), b"notes").unwrap();
    let failed = nightly.join("domovoi-20260103T040000Z.db");
    std::fs::write(&failed, b"would not verify").unwrap();

    let err = verify_or_tracked_for_tests(&failed, "the database failed its integrity check")
        .await
        .unwrap_err();
    match err {
        StoreOpenError::Verify { path, reason } => {
            assert_eq!(path, failed);
            assert!(
                reason.contains("failed its integrity check"),
                "the check's reason is kept: {reason}"
            );
        }
        other => panic!("expected Verify, got {other:?}"),
    }
    assert!(!failed.exists(), "the failed copy was removed");
    assert_eq!(
        names(&nightly),
        [
            "domovoi-20260101T040000Z.db",
            "domovoi-20260102T040000Z.db",
            "notes.txt"
        ],
        "the good backups and the unrelated file are untouched"
    );
}

#[tokio::test]
async fn a_backup_that_cannot_be_removed_says_so() {
    let dir = tempfile::tempdir().unwrap();
    // A directory cannot be removed as a file, so the cleanup itself fails.
    let stuck = dir.path().join("not-a-file.db");
    std::fs::create_dir_all(&stuck).unwrap();

    let err = verify_or_tracked_for_tests(&stuck, "damaged")
        .await
        .unwrap_err();
    let StoreOpenError::Verify { reason, .. } = err else {
        panic!("expected Verify, got {err:?}");
    };
    assert!(
        reason.contains("could not be removed"),
        "the failure to remove is reported, not hidden: {reason}"
    );
    assert!(stuck.exists(), "nothing was deleted");
}
