//! Resolution summaries on SQLite: the shared contract suite, and what SQLite stores beyond it.

use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;
use domovoi_core::experience::{ExperienceQuery, ExperienceScope, SummaryField, SummarySource};
use domovoi_core::ports::{
    Expected, ExperienceRead, OperationWrite, StateWrite, SummaryFieldWrite, Transaction,
    UnitOfWork,
};
use domovoi_core::{
    Budgets, CheckKey, HostId, Operation, OperationId, OperationKind, OperationStatus,
    PinnedSubject, TargetKey,
};
use domovoi_store::{MIN_READER, Store, StoreOptions};
use domovoi_testkit::build::at;
use domovoi_testkit::contracts::ExperienceSubject;
use serde_json::{Value, json};
use sqlx::ConnectOptions;

use crate::common::Fresh;

struct SqliteExperience(Store);

#[async_trait]
impl ExperienceSubject for SqliteExperience {
    fn uow(&self) -> &dyn UnitOfWork {
        &self.0
    }

    fn experience(&self) -> &dyn ExperienceRead {
        &self.0
    }
}

macro_rules! contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let fresh = Fresh::new().await;
                domovoi_testkit::contracts::$suite(&SqliteExperience(fresh.store.clone()))
                    .await
                    .unwrap();
            }
        )*
    };
}

contract_tests! {
    sqlite_first_revision_must_be_the_outcome => first_revision_must_be_the_outcome,
    sqlite_a_summary_needs_a_finished_operation => a_summary_needs_a_finished_operation,
    sqlite_revisions_are_dense_and_a_stale_revision_conflicts => revisions_are_dense_and_a_stale_revision_conflicts,
    sqlite_source_must_match_field_ownership => source_must_match_field_ownership,
    sqlite_current_returns_the_newest_revision_of_each_field_in_order => current_returns_the_newest_revision_of_each_field_in_order,
    sqlite_the_projection_equals_the_fold_of_the_revisions => the_projection_equals_the_fold_of_the_revisions,
    sqlite_search_is_host_scoped => search_is_host_scoped,
    sqlite_search_returns_all_word_matches_before_any_word_matches => search_returns_all_word_matches_before_any_word_matches,
    sqlite_search_with_no_words_lists_the_newest_first => search_with_no_words_lists_the_newest_first,
    sqlite_search_never_reads_user_text_as_syntax => search_never_reads_user_text_as_syntax,
    sqlite_search_excludes_the_given_operation => search_excludes_the_given_operation,
    sqlite_search_sees_only_the_current_text => search_sees_only_the_current_text,
    sqlite_search_finds_outcome_tags => search_finds_outcome_tags,
    sqlite_similar_scores_check_target_contract_and_words => similar_scores_check_target_contract_and_words,
    sqlite_similar_stays_on_one_host_and_never_returns_the_excluded_operation => similar_stays_on_one_host_and_never_returns_the_excluded_operation,
    sqlite_undrafted_lists_only_finished_operations_with_no_revision => undrafted_lists_only_finished_operations_with_no_revision,
    sqlite_briefs_cover_only_operations_that_have_a_summary => briefs_cover_only_operations_that_have_a_summary,
}

fn revision(
    operation: OperationId,
    field: SummaryField,
    expected: u32,
    value: Value,
    source: SummarySource,
) -> StateWrite {
    StateWrite::Operation(OperationWrite::SummaryField(SummaryFieldWrite {
        operation,
        field,
        expected_revision: expected,
        value,
        source,
        actor: "system:operations".to_owned(),
        at: at(100),
        tags: None,
    }))
}

/// Stores a finished operation on `host` with an outcome, hosts, subjects and a problem text.
async fn finished(store: &Store, host: HostId, problem: &str) -> OperationId {
    let mut operation = Operation::queued(
        OperationId::new(),
        OperationKind::Investigate,
        "check the pool",
        vec![PinnedSubject {
            host,
            check: CheckKey::parse("pool.degraded").unwrap(),
            target: Some(TargetKey::parse("tank").unwrap()),
        }],
        None,
        Budgets::default(),
        at(10),
    )
    .unwrap();
    operation.status = OperationStatus::Succeeded;
    operation.ended_at = Some(at(20));
    let id = operation.id;
    let tx = Transaction::new()
        .write(StateWrite::Operation(OperationWrite::Operation {
            operation,
            expected: Expected::New,
        }))
        .write(revision(
            id,
            SummaryField::Outcome,
            0,
            json!("succeeded"),
            SummarySource::Code,
        ))
        .write(revision(
            id,
            SummaryField::Hosts,
            0,
            json!([host]),
            SummarySource::Code,
        ))
        .write(revision(
            id,
            SummaryField::Subjects,
            0,
            json!([{"host": host, "check": "pool.degraded", "target": "tank"}]),
            SummarySource::Code,
        ))
        .write(revision(
            id,
            SummaryField::Problem,
            0,
            json!(problem),
            SummarySource::Model,
        ));
    store.commit(tx).await.unwrap();
    id
}

async fn count(raw: &mut sqlx::SqliteConnection, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM \"{table}\"");
    sqlx::query_as::<_, (i64,)>(sqlx::AssertSqlSafe(sql))
        .fetch_one(raw)
        .await
        .unwrap()
        .0
}

async fn table_counts(path: &Path) -> BTreeMap<String, i64> {
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .unwrap();
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         AND name NOT LIKE '%\\_fts%' ESCAPE '\\'",
    )
    .fetch_all(&mut raw)
    .await
    .unwrap();
    let mut counts = BTreeMap::new();
    for (name,) in names {
        let n = count(&mut raw, &name).await;
        counts.insert(name, n);
    }
    counts
}

#[tokio::test]
async fn a_fresh_database_has_empty_summary_tables_and_passes_the_integrity_checks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db").join("domovoi.db");
    let store = Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap();
    let after = table_counts(&path).await;
    for table in [
        "resolution_summary_fields",
        "operation_resolutions",
        "operation_resolution_hosts",
        "operation_resolution_subjects",
    ] {
        assert_eq!(after.get(table), Some(&0), "{table} should be empty");
    }
    let pool = store.pool_for_tests();
    let fts: (i64,) = sqlx::query_as("SELECT count(*) FROM operation_resolutions_fts")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(fts.0, 0);
    let check: (String,) = sqlx::query_as("PRAGMA quick_check")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(check.0, "ok");
    let fk: Vec<(i64,)> = sqlx::query_as("SELECT 1 FROM pragma_foreign_key_check")
        .fetch_all(pool)
        .await
        .unwrap();
    assert!(fk.is_empty());
    let (min_reader,): (String,) =
        sqlx::query_as("SELECT min_reader FROM schema_compat WHERE id = 1")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(min_reader, MIN_READER);
    assert!(
        store
            .briefs(&[OperationId::new()])
            .await
            .unwrap()
            .is_empty()
    );
    let hits = store
        .search(&ExperienceQuery {
            scope: ExperienceScope::Global,
            text: "anything".to_owned(),
            limit: 5,
            exclude: None,
        })
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn a_finished_operation_without_a_summary_is_listed_as_undrafted() {
    let fresh = Fresh::new().await;
    let pool = fresh.store.pool_for_tests();
    sqlx::query(
        "INSERT INTO operations (id, kind, goal, status, cycle, budgets, pinned, created_at, \
         ended_at, updated_at) VALUES ('01900000-0000-7000-8000-0000000000aa', 'investigate', 'old', 'failed', 0, '{}', '[]', 1, 2, 2)",
    )
    .execute(pool)
    .await
    .unwrap();
    let listed = fresh.store.undrafted(10).await.unwrap();
    let expected: OperationId = "01900000-0000-7000-8000-0000000000aa".parse().unwrap();
    assert_eq!(listed, [expected]);
}

#[tokio::test]
async fn summary_revisions_cannot_be_updated_or_deleted() {
    let fresh = Fresh::new().await;
    let id = finished(&fresh.store, HostId::new(), "zfs degraded").await;
    let pool = fresh.store.pool_for_tests();
    for sql in [
        "UPDATE resolution_summary_fields SET actor = 'x'",
        "DELETE FROM resolution_summary_fields",
    ] {
        let error = sqlx::query(sql)
            .execute(pool)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("summary revisions are append-only"),
            "{error}"
        );
    }
    assert_eq!(fresh.store.revisions(id).await.unwrap().len(), 4);
}

#[tokio::test]
async fn the_full_text_index_passes_its_integrity_check_after_edits() {
    let fresh = Fresh::new().await;
    let id = finished(&fresh.store, HostId::new(), "zfs degraded").await;
    let edit = Transaction::new().write(revision(
        id,
        SummaryField::Problem,
        1,
        json!("kernel panic"),
        SummarySource::User,
    ));
    fresh.store.commit(edit).await.unwrap();
    sqlx::query(
        "INSERT INTO operation_resolutions_fts(operation_resolutions_fts) VALUES('integrity-check')",
    )
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
}

#[tokio::test]
async fn the_full_text_table_keeps_its_content_table_text() {
    let fresh = Fresh::new().await;
    let (sql,): (String,) =
        sqlx::query_as("SELECT sql FROM sqlite_master WHERE name = 'operation_resolutions_fts'")
            .fetch_one(fresh.store.pool_for_tests())
            .await
            .unwrap();
    assert!(sql.contains("content = 'operation_resolutions'"), "{sql}");
}

#[tokio::test]
async fn summaries_survive_pruning_of_outputs_and_journal_rows() {
    let fresh = Fresh::new().await;
    let host = HostId::new();
    let id = finished(&fresh.store, host, "zfs degraded").await;
    let query = ExperienceQuery {
        scope: ExperienceScope::Host(host),
        text: "zfs".to_owned(),
        limit: 5,
        exclude: None,
    };
    let before = fresh.store.search(&query).await.unwrap();
    assert_eq!(before.len(), 1);
    let pool = fresh.store.pool_for_tests();
    sqlx::query("UPDATE steps SET output = NULL")
        .execute(pool)
        .await
        .unwrap();
    fresh
        .store
        .prune_journal_through(u64::MAX >> 2)
        .await
        .unwrap();
    let after = fresh.store.search(&query).await.unwrap();
    assert_eq!(after, before);
    assert!(fresh.store.summary(id).await.unwrap().is_some());
}

#[tokio::test]
async fn host_scoped_search_uses_the_host_index() {
    let fresh = Fresh::new().await;
    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT r.operation_id FROM operation_resolutions r WHERE \
         EXISTS (SELECT 1 FROM operation_resolution_hosts x WHERE x.operation_id = r.operation_id \
         AND x.host_id = ?1)",
    )
    .bind("h")
    .fetch_all(fresh.store.pool_for_tests())
    .await
    .unwrap();
    let text: String = plan
        .iter()
        .map(|row| row.3.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("SEARCH x"), "{text}");
    assert!(!text.contains("SCAN x"), "{text}");
}

#[tokio::test]
async fn a_failed_commit_leaves_no_revision_projection_or_index_row() {
    let fresh = Fresh::new().await;
    let host = HostId::new();
    let id = finished(&fresh.store, host, "zfs degraded").await;
    // The second write names a stale revision, so the whole transaction rolls back.
    let tx = Transaction::new()
        .write(revision(
            id,
            SummaryField::Problem,
            1,
            json!("kernel panic"),
            SummarySource::User,
        ))
        .write(revision(
            id,
            SummaryField::Problem,
            1,
            json!("again"),
            SummarySource::User,
        ));
    assert!(fresh.store.commit(tx).await.is_err());
    let revisions = fresh.store.revisions(id).await.unwrap();
    assert_eq!(
        revisions
            .iter()
            .filter(|r| r.field == SummaryField::Problem)
            .count(),
        1
    );
    let hits = fresh
        .store
        .search(&ExperienceQuery {
            scope: ExperienceScope::Host(host),
            text: "kernel".to_owned(),
            limit: 5,
            exclude: None,
        })
        .await
        .unwrap();
    assert!(hits.is_empty());
}
