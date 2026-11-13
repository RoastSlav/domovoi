//! The audit hash chain: anchor, chained rows, verification and the tamper cases it detects.

use std::path::{Path, PathBuf};

use domovoi_core::audit_chain::{
    BreakReason, CHECKPOINT_KIND, ChainStatus, Hash, LegacyDigest, LegacyStatus, VerifyScope,
    entry_hash,
};
use domovoi_core::ports::{
    AuditDraft, AuditEvent, AuditFilter, AuditLog, Expected, Page, StateWrite, Transaction,
    UnitOfWork,
};
use domovoi_core::{Actor, IncidentId};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{at, host, incident};
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::common::Fresh;

const NO_UPDATE: &str = "CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN \
    SELECT RAISE(ABORT, 'audit_log is append-only'); END";
const NO_DELETE: &str = "CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log BEGIN \
    SELECT RAISE(ABORT, 'audit_log is append-only'); END";
const NO_ANCHOR_UPDATE: &str = "CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN \
    SELECT RAISE(ABORT, 'audit_anchor is append-only'); END";

fn event(kind: &str, at_secs: i64) -> AuditEvent {
    AuditEvent {
        at: at(at_secs),
        actor: "system:test".to_owned(),
        kind: kind.to_owned(),
        subject: "subject".to_owned(),
        detail: Value::Null,
    }
}

async fn append(store: &Store, kind: &str, at_secs: i64) -> u64 {
    AuditLog::append(store, &event(kind, at_secs))
        .await
        .unwrap()
}

async fn drop_trigger(pool: &SqlitePool, name: &str) {
    let sql = format!("DROP TRIGGER {name}");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .unwrap();
}

async fn recreate_trigger(pool: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}

async fn stored_hash(pool: &SqlitePool, seq: u64) -> (Vec<u8>, Vec<u8>) {
    sqlx::query_as("SELECT prev_hash, hash FROM audit_log WHERE seq = ?")
        .bind(i64::try_from(seq).unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("n1-schema5.db")
}

fn copy_fixture(dir: &Path) -> PathBuf {
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).unwrap();
    let path = dbdir.join("domovoi.db");
    std::fs::copy(fixture(), &path).unwrap();
    path
}

async fn open_fixture(dir: &Path) -> Store {
    let path = copy_fixture(dir);
    Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap()
}

async fn append_checkpoint(store: &Store, first_seq: u64, prev: &Hash, legacy_pruned: bool) -> u64 {
    AuditLog::append(
        store,
        &AuditEvent {
            at: at(100),
            actor: "system:audit".to_owned(),
            kind: CHECKPOINT_KIND.to_owned(),
            subject: "audit".to_owned(),
            detail: json!({
                "first_retained_seq": first_seq,
                "first_retained_prev_hash": hex::encode(prev),
                "pruned_count": first_seq.saturating_sub(1),
                "legacy_pruned": legacy_pruned,
            }),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn fresh_install_gets_an_empty_anchor() {
    let fresh = Fresh::new().await;
    let anchor = fresh.store.audit_anchor().await.unwrap().unwrap();
    assert_eq!(anchor.legacy_last_id, 0);
    assert_eq!(anchor.legacy_count, 0);
    assert_eq!(anchor.legacy_digest, LegacyDigest::new().finish().1);
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_anchor")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn first_chained_row_links_to_the_anchor() {
    let fresh = Fresh::new().await;
    let anchor = fresh.store.audit_anchor().await.unwrap().unwrap();
    let seq = append(&fresh.store, "test.first", 1).await;
    assert_eq!(seq, 1);
    let (prev, hash) = stored_hash(fresh.store.pool_for_tests(), 1).await;
    assert_eq!(prev, anchor.hash().to_vec());
    assert_eq!(
        hash,
        entry_hash(1, &anchor.hash(), &event("test.first", 1)).to_vec()
    );
}

#[tokio::test]
async fn rows_written_through_a_commit_and_through_append_share_one_chain() {
    let fresh = Fresh::new().await;
    for index in 0..4_i64 {
        if index % 2 == 0 {
            append(&fresh.store, &format!("test.{index}"), index).await;
        } else {
            let draft = AuditDraft::new(
                Actor::System("test".to_owned()),
                at(index),
                format!("test.{index}"),
                "subject".to_owned(),
                Value::Null,
            );
            UnitOfWork::commit(&fresh.store, Transaction::new().audit(draft))
                .await
                .unwrap();
        }
    }
    let seqs: Vec<i64> = sqlx::query_scalar("SELECT seq FROM audit_log ORDER BY seq")
        .fetch_all(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Intact { checked: 4, .. }
    ));
}

#[tokio::test]
async fn audit_failure_rolls_back_the_chain_position() {
    let fresh = Fresh::new().await;
    let anchor = fresh.store.audit_anchor().await.unwrap().unwrap();
    let host = host("nas").unwrap();
    let ghost = incident(host.id, "load.high", None)
        .unwrap()
        .with_id(IncidentId::new());
    let draft = AuditDraft::new(
        Actor::System("test".to_owned()),
        at(1),
        "test.failed".to_owned(),
        "subject".to_owned(),
        Value::Null,
    );
    let failed = UnitOfWork::commit(
        &fresh.store,
        Transaction::new()
            .write(StateWrite::Incident {
                incident: ghost,
                expected: Expected::Version(7),
            })
            .audit(draft),
    )
    .await;
    assert!(failed.is_err());
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(count.0, 0);

    assert_eq!(append(&fresh.store, "test.after", 2).await, 1);
    let (prev, _) = stored_hash(fresh.store.pool_for_tests(), 1).await;
    assert_eq!(prev, anchor.hash().to_vec());
}

#[tokio::test]
async fn audit_chain_detects_edit() {
    let fresh = Fresh::new().await;
    for index in 1..=3 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_update").await;
    sqlx::query("UPDATE audit_log SET detail = '{\"edited\":true}' WHERE seq = 2")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_UPDATE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 2,
            reason: BreakReason::HashMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn audit_chain_detects_mid_chain_deletion() {
    let fresh = Fresh::new().await;
    for index in 1..=3 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_delete").await;
    sqlx::query("DELETE FROM audit_log WHERE seq = 2")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_DELETE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 3,
            reason: BreakReason::SequenceGap,
            ..
        }
    ));
}

#[tokio::test]
async fn audit_chain_detects_a_row_inserted_without_the_chain() {
    let fresh = Fresh::new().await;
    append(&fresh.store, "test.1", 1).await;
    append(&fresh.store, "test.2", 2).await;
    sqlx::query(
        "INSERT INTO audit_log (seq, at, actor, kind, subject, detail) \
         VALUES (3, 0, 'tool', 'tool.insert', 'subject', 'null')",
    )
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 3,
            reason: BreakReason::MissingChainFields,
            ..
        }
    ));
}

#[tokio::test]
async fn legacy_audit_digest_reported_separately() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_fixture(dir.path()).await;
    let pool = store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_update").await;
    sqlx::query("UPDATE audit_log SET detail = '{\"edited\":true}' WHERE seq = 1")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_UPDATE).await;

    let report = store.verify_audit(VerifyScope::Full).await.unwrap();
    assert!(matches!(
        report.legacy,
        LegacyStatus::Mismatch {
            expected_count: 5,
            actual_count: 5,
            digest_matches: false,
        }
    ));
    assert_eq!(report.chained, ChainStatus::Empty);
}

#[tokio::test]
async fn last_n_verification_checks_only_the_newest_rows() {
    let fresh = Fresh::new().await;
    for index in 1..=30 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_update").await;
    sqlx::query("UPDATE audit_log SET detail = '{\"edited\":true}' WHERE seq = 5")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_UPDATE).await;

    let last = fresh
        .store
        .verify_audit(VerifyScope::Last(10))
        .await
        .unwrap();
    assert!(matches!(
        last.chained,
        ChainStatus::Intact { checked: 10, .. }
    ));
    assert_eq!(last.legacy, LegacyStatus::CountOnly { count: 0 });

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 5,
            reason: BreakReason::HashMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn verification_starts_at_the_retained_start_after_a_checkpoint() {
    let fresh = Fresh::new().await;
    for index in 1..=10 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    let (prev,): (Vec<u8>,) = sqlx::query_as("SELECT prev_hash FROM audit_log WHERE seq = 5")
        .fetch_one(pool)
        .await
        .unwrap();
    let prev: Hash = prev.try_into().unwrap();
    append_checkpoint(&fresh.store, 5, &prev, false).await;

    drop_trigger(pool, "audit_log_no_delete").await;
    sqlx::query("DELETE FROM audit_log WHERE seq < 5")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_DELETE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Intact { checked: 7, .. }
    ));
}

#[tokio::test]
async fn checkpoint_with_a_wrong_link_is_broken() {
    let fresh = Fresh::new().await;
    for index in 1..=10 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    append_checkpoint(&fresh.store, 5, &[0u8; 32], false).await;
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_delete").await;
    sqlx::query("DELETE FROM audit_log WHERE seq < 5")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_DELETE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 5,
            reason: BreakReason::CheckpointMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn newest_checkpoint_for_a_retained_row_wins() {
    let fresh = Fresh::new().await;
    for index in 1..=10 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    let (prev,): (Vec<u8>,) = sqlx::query_as("SELECT prev_hash FROM audit_log WHERE seq = 5")
        .fetch_one(pool)
        .await
        .unwrap();
    let prev: Hash = prev.try_into().unwrap();
    append_checkpoint(&fresh.store, 5, &prev, false).await;
    append_checkpoint(&fresh.store, 5, &[0u8; 32], false).await;

    drop_trigger(pool, "audit_log_no_delete").await;
    sqlx::query("DELETE FROM audit_log WHERE seq < 5")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_DELETE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 5,
            reason: BreakReason::CheckpointMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn pruning_without_a_checkpoint_is_broken() {
    let fresh = Fresh::new().await;
    for index in 1..=10 {
        append(&fresh.store, &format!("test.{index}"), index).await;
    }
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_log_no_delete").await;
    sqlx::query("DELETE FROM audit_log WHERE seq < 5")
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_DELETE).await;

    assert!(matches!(
        fresh
            .store
            .verify_audit(VerifyScope::Full)
            .await
            .unwrap()
            .chained,
        ChainStatus::Broken {
            first_broken_seq: 5,
            reason: BreakReason::CheckpointMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn edited_anchor_hash_is_rejected_by_verify_and_append() {
    let fresh = Fresh::new().await;
    let pool = fresh.store.pool_for_tests();
    drop_trigger(pool, "audit_anchor_no_update").await;
    sqlx::query("UPDATE audit_anchor SET anchor_hash = ?")
        .bind(vec![0u8; 32])
        .execute(pool)
        .await
        .unwrap();
    recreate_trigger(pool, NO_ANCHOR_UPDATE).await;

    assert!(
        fresh.store.verify_audit(VerifyScope::Full).await.is_err(),
        "verification must not accept an edited anchor"
    );
    assert!(
        AuditLog::append(&fresh.store, &event("test.after", 1))
            .await
            .is_err(),
        "append must not extend a chain from an edited anchor"
    );
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[tokio::test]
async fn audit_anchor_cannot_be_changed() {
    let fresh = Fresh::new().await;
    let pool = fresh.store.pool_for_tests();
    for sql in [
        "UPDATE audit_anchor SET install_id = 'other'",
        "DELETE FROM audit_anchor",
    ] {
        let err = sqlx::query(sql).execute(pool).await.unwrap_err();
        assert!(
            err.to_string().contains("audit_anchor is append-only"),
            "{sql}: {err}"
        );
    }
}

#[tokio::test]
async fn audit_head_reports_the_newest_chained_row() {
    let fresh = Fresh::new().await;
    assert_eq!(fresh.store.audit_head().await.unwrap(), None);
    append(&fresh.store, "test.1", 1).await;
    let seq = append(&fresh.store, "test.2", 2).await;
    let (_, hash) = stored_hash(fresh.store.pool_for_tests(), seq).await;
    let hash: Hash = hash.try_into().unwrap();
    assert_eq!(fresh.store.audit_head().await.unwrap(), Some((seq, hash)));
}

#[tokio::test]
async fn timestamps_are_hashed_at_millisecond_precision() {
    let fresh = Fresh::new().await;
    let precise: jiff::Timestamp = "2023-11-14T22:13:20.123456789Z".parse().unwrap();
    let event = AuditEvent {
        at: precise,
        actor: "system:test".to_owned(),
        kind: "test.precise".to_owned(),
        subject: "subject".to_owned(),
        detail: json!({"n": 1}),
    };
    let seq = AuditLog::append(&fresh.store, &event).await.unwrap();
    let (_, hash) = stored_hash(fresh.store.pool_for_tests(), seq).await;
    let listed = fresh
        .store
        .list(&AuditFilter::default(), &Page::first(10))
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(listed.at.as_millisecond(), 1_700_000_000_123);
    let anchor = fresh.store.audit_anchor().await.unwrap().unwrap();
    assert_eq!(
        hash,
        entry_hash(seq, &anchor.hash(), &listed).to_vec(),
        "the stored hash covers the millisecond timestamp"
    );
}

#[tokio::test]
async fn insert_into_audit_log_exists_in_one_source_file() {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    let mut hits: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap()
                .contains("INSERT INTO audit_log")
        })
        .collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits.remove(0), src.join("audit_chain.rs"));
}
