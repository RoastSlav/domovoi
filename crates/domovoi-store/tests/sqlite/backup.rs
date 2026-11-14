//! Backups are verified before older copies go, and pre-migration copies are pruned.

use domovoi_core::ports::AdvisoryRepo;
use domovoi_core::{Advisory, AdvisoryId, AdvisoryKind};
use domovoi_store::{Depth, MIGRATOR, Store, verify_backup};
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
