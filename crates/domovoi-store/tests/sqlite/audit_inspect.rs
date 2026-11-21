//! `AuditInspect` on SQLite: rows with their chain fields, history split, and read-only reads.

use std::path::Path;

use domovoi_core::audit_chain::{BreakReason, ChainStatus, VerifyScope};
use domovoi_core::ports::{
    AuditEvent, AuditFilter, AuditHistory, AuditInspect, AuditLog, AuditRowFilter, Page, StoreError,
};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::at;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::common::Fresh;

const NO_UPDATE: &str = "CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN \
    SELECT RAISE(ABORT, 'audit_log is append-only'); END";
const NO_ANCHOR_UPDATE: &str = "CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN \
    SELECT RAISE(ABORT, 'audit_anchor is append-only'); END";

async fn append(store: &Store, kind: &str) -> u64 {
    let event = AuditEvent {
        at: at(100),
        actor: "system:test".to_owned(),
        kind: kind.to_owned(),
        subject: "subject".to_owned(),
        detail: Value::Null,
    };
    AuditLog::append(store, &event).await.unwrap()
}

async fn exec(pool: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}

async fn open_fixture(dir: &Path) -> Store {
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).unwrap();
    let path = dbdir.join("domovoi.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("n1-schema5.db"),
        &path,
    )
    .unwrap();
    Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap()
}

fn seqs(rows: &[domovoi_core::audit_chain::ChainRow]) -> Vec<u64> {
    rows.iter().map(|r| r.seq).collect()
}

fn history(h: AuditHistory) -> AuditRowFilter {
    AuditRowFilter {
        history: Some(h),
        ..AuditRowFilter::default()
    }
}

#[tokio::test]
async fn rows_come_newest_first_with_their_chain_fields() {
    let f = Fresh::new().await;
    for _ in 0..3 {
        append(&f.store, "k").await;
    }
    let page = f
        .store
        .rows(&AuditRowFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(seqs(&page.items), [3, 2, 1]);
    assert!(
        page.items
            .iter()
            .all(|r| r.hash.is_some() && r.prev_hash.is_some())
    );
    let anchor = f.store.anchor().await.unwrap().unwrap();
    assert_eq!(page.items[2].prev_hash, Some(anchor.hash()));
    assert_eq!(page.items[1].prev_hash, page.items[2].hash);
    assert_eq!(page.items[0].prev_hash, page.items[1].hash);
}

#[tokio::test]
async fn rows_page_with_a_cursor_until_the_last_page_has_none() {
    let f = Fresh::new().await;
    for _ in 0..5 {
        append(&f.store, "k").await;
    }
    let filter = AuditRowFilter::default();
    let mut page = Page::first(2);
    let mut got = Vec::new();
    loop {
        let out = f.store.rows(&filter, &page).await.unwrap();
        got.push(seqs(&out.items));
        match out.next_cursor {
            Some(c) => {
                page = Page {
                    cursor: Some(c),
                    ..Page::first(2)
                }
            }
            None => break,
        }
    }
    assert_eq!(got, [vec![5, 4], vec![3, 2], vec![1]]);
}

#[tokio::test]
async fn history_filter_splits_legacy_from_chained() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_fixture(dir.path()).await;
    let first = append(&store, "new.a").await;
    let second = append(&store, "new.b").await;
    let page = Page::first(50);
    let legacy = store
        .rows(&history(AuditHistory::Legacy), &page)
        .await
        .unwrap();
    assert_eq!(seqs(&legacy.items), [5, 4, 3, 2, 1]);
    assert!(legacy.items.iter().all(|r| r.hash.is_none()));
    let chained = store
        .rows(&history(AuditHistory::Chained), &page)
        .await
        .unwrap();
    assert_eq!(seqs(&chained.items), [second, first]);
    assert!(chained.items.iter().all(|r| r.hash.is_some()));
    let all = store.rows(&AuditRowFilter::default(), &page).await.unwrap();
    assert_eq!(all.items.len(), 7);
}

#[tokio::test]
async fn kind_filter_narrows_the_rows_and_combines_with_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_fixture(dir.path()).await;
    append(&store, "new.a").await;
    let b = append(&store, "new.b").await;
    let filter = AuditRowFilter {
        events: AuditFilter {
            kind: Some("new.b".to_owned()),
            ..AuditFilter::default()
        },
        history: Some(AuditHistory::Chained),
    };
    let out = store.rows(&filter, &Page::first(50)).await.unwrap();
    assert_eq!(seqs(&out.items), [b]);
    let none = AuditRowFilter {
        history: Some(AuditHistory::Legacy),
        ..filter
    };
    assert!(
        store
            .rows(&none, &Page::first(50))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn a_chained_row_that_lost_its_hash_is_still_chained_and_breaks_the_chain() {
    let f = Fresh::new().await;
    append(&f.store, "k").await;
    append(&f.store, "k").await;
    let pool = f.store.pool_for_tests();
    exec(pool, "DROP TRIGGER audit_log_no_update").await;
    exec(pool, "UPDATE audit_log SET hash = NULL WHERE seq = 2").await;
    exec(pool, NO_UPDATE).await;
    let page = Page::first(10);
    let chained = f
        .store
        .rows(&history(AuditHistory::Chained), &page)
        .await
        .unwrap();
    let row = chained.items.iter().find(|r| r.seq == 2).unwrap();
    assert!(row.hash.is_none());
    let legacy = f
        .store
        .rows(&history(AuditHistory::Legacy), &page)
        .await
        .unwrap();
    assert!(legacy.items.is_empty());
    let report = f.store.verify(VerifyScope::Full).await.unwrap();
    assert!(matches!(
        report.chained,
        ChainStatus::Broken {
            reason: BreakReason::MissingChainFields,
            ..
        }
    ));
}

#[tokio::test]
async fn anchor_head_and_verify_agree_with_the_store_methods() {
    let f = Fresh::new().await;
    append(&f.store, "k").await;
    append(&f.store, "k").await;
    assert_eq!(
        f.store.anchor().await.unwrap(),
        f.store.audit_anchor().await.unwrap()
    );
    assert_eq!(
        f.store.head().await.unwrap(),
        f.store.audit_head().await.unwrap()
    );
    assert_eq!(
        f.store.verify(VerifyScope::Last(1)).await.unwrap(),
        f.store.verify_audit(VerifyScope::Last(1)).await.unwrap()
    );
}

#[tokio::test]
async fn an_edited_anchor_is_an_error_not_a_report() {
    let f = Fresh::new().await;
    append(&f.store, "k").await;
    let pool = f.store.pool_for_tests();
    exec(pool, "DROP TRIGGER audit_anchor_no_update").await;
    exec(pool, "UPDATE audit_anchor SET install_id = 'edited'").await;
    exec(pool, NO_ANCHOR_UPDATE).await;
    assert!(matches!(
        f.store.anchor().await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        f.store.verify(VerifyScope::Full).await,
        Err(StoreError::Backend(_))
    ));
    let rows = f
        .store
        .rows(&AuditRowFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(rows.items.len(), 1);
}

#[tokio::test]
async fn reading_the_audit_rows_writes_nothing() {
    let f = Fresh::new().await;
    append(&f.store, "k").await;
    let count = || async {
        let a: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log")
            .fetch_one(f.store.pool_for_tests())
            .await
            .unwrap();
        let b: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_anchor")
            .fetch_one(f.store.pool_for_tests())
            .await
            .unwrap();
        (a.0, b.0)
    };
    let before = count().await;
    f.store
        .rows(&AuditRowFilter::default(), &Page::first(10))
        .await
        .unwrap();
    f.store.anchor().await.unwrap();
    f.store.head().await.unwrap();
    f.store.verify(VerifyScope::Full).await.unwrap();
    f.store.verify(VerifyScope::Last(5)).await.unwrap();
    assert_eq!(count().await, before);
}
