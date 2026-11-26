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
