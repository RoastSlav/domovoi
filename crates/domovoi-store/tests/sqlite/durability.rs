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

#[cfg(unix)]
#[tokio::test]
async fn the_database_and_its_wal_files_are_readable_by_the_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let fresh = crate::common::Fresh::new().await;
    let path = fresh.path();
    // A database an older version left world-readable is tightened on the next open.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    fresh.store.close().await;
    let reopened = domovoi_store::Store::open(
        &path,
        domovoi_store::StoreOptions::beside(&path, domovoi_testkit::build::T0),
    )
    .await
    .unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.clone().into_os_string();
        name.push(suffix);
        if let Ok(meta) = std::fs::metadata(&name) {
            assert_eq!(
                meta.permissions().mode() & 0o077,
                0,
                "{name:?} is open to group or others"
            );
        }
    }
    reopened.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_data_directory_anyone_can_enter_loses_the_other_bits_on_open() {
    use std::os::unix::fs::PermissionsExt;
    let fresh = crate::common::Fresh::new().await;
    let path = fresh.path();
    let dir = path.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    fresh.store.close().await;
    let reopened = domovoi_store::Store::open(
        &path,
        domovoi_store::StoreOptions::beside(&path, domovoi_testkit::build::T0),
    )
    .await
    .unwrap();
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o007, 0, "others can still enter the directory");
    assert_eq!(mode & 0o750, 0o750, "the owner and group keep their access");
    reopened.close().await;
}

#[tokio::test]
async fn readiness_fails_when_the_database_cannot_take_a_write() {
    use domovoi_core::ports::{StoreError, StoreHealth};

    let fresh = Fresh::new().await;
    fresh.store.writable().await.unwrap();
    // The one writer connection now refuses every write, as a read-only file would.
    sqlx::query("PRAGMA query_only = 1")
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    let err = fresh.store.writable().await.unwrap_err();
    assert!(
        matches!(err, StoreError::Unwritable(_)),
        "a store that cannot write answered {err:?}"
    );
    fresh.store.close().await;
}

#[tokio::test]
async fn a_full_database_is_reported_as_unwritable_not_as_a_generic_failure() {
    use domovoi_core::ports::{SettingsRepo, StoreError};

    let fresh = Fresh::new().await;
    // A database that may not grow any more: the same SQLITE_FULL a full disk gives.
    let (pages,): (i64,) = sqlx::query_as("PRAGMA page_count")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA max_page_count = {pages}"
    )))
    .fetch_all(fresh.store.pool_for_tests())
    .await
    .unwrap();
    let big = serde_json::Value::String("x".repeat(400_000));
    let err = fresh
        .store
        .set_many(&[("probe.big".to_owned(), Some(big))])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, StoreError::Unwritable(reason) if reason.contains("full")),
        "a full database answered {err:?}"
    );
    fresh.store.close().await;
}

/// Fills the file system holding `dir` until a write fails.
fn fill_disk(dir: &std::path::Path) {
    use std::io::Write;

    let mut filler = std::fs::File::create(dir.join("filler")).unwrap();
    let chunk = vec![0u8; 4096];
    while filler
        .write_all(&chunk)
        .and_then(|()| filler.flush())
        .is_ok()
    {}
}

/// The real thing: a store on a file system with no space left. It needs a small file system
/// that the test may fill, so it only runs on request:
/// `unshare -rm sh -c 'mount -t tmpfs -o size=2m tmpfs /mnt/small && DOMOVOI_FULL_DISK_DIR=/mnt/small cargo test -p domovoi-store --test sqlite -- --ignored full_disk'`.
#[tokio::test]
#[ignore = "needs a small file system to fill: set DOMOVOI_FULL_DISK_DIR"]
async fn readiness_fails_on_a_file_system_with_no_space_left() {
    use domovoi_core::ports::{StoreError, StoreHealth};
    use domovoi_store::{Store, StoreOptions};
    use domovoi_testkit::build::T0;

    let dir = std::env::var("DOMOVOI_FULL_DISK_DIR").expect("DOMOVOI_FULL_DISK_DIR");
    let dir = std::path::Path::new(&dir).join("full-disk-test");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("domovoi.db");
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    store.writable().await.unwrap();
    fill_disk(&dir);
    let err = store.writable().await.unwrap_err();
    assert!(
        matches!(err, StoreError::Unwritable(_)),
        "a full disk answered {err:?}"
    );
    std::fs::remove_file(dir.join("filler")).unwrap();
    store.writable().await.unwrap();
    store.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
