//! What SQLite guarantees for the operation tables beyond the shared contracts: the
//! triggers that guard raw writes, and unreadable stored names.

use domovoi_core::ports::{OperationRepo, StoreError};
use domovoi_core::{Actor, Budgets, OperationId, StepId, StepIntent};
use domovoi_store::{MIN_READER, Store};

use crate::common::Fresh;

const NEW_TABLES: [&str; 5] = [
    "operations",
    "operation_incidents",
    "steps",
    "step_edges",
    "executions",
];

async fn count(conn: &mut sqlx::SqliteConnection, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table}");
    let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_one(conn)
        .await
        .unwrap();
    row.0
}

#[tokio::test]
async fn a_fresh_database_has_the_empty_operation_tables() {
    let fresh = Fresh::new().await;
    let mut conn = fresh.store.pool_for_tests().acquire().await.unwrap();
    for table in NEW_TABLES {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_optional(&mut *conn)
                .await
                .unwrap();
        assert!(found.is_some(), "{table} exists");
        assert_eq!(count(&mut conn, table).await, 0, "{table} starts empty");
    }
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert!(violations.is_empty());
    let compat: Vec<(i64, String)> = sqlx::query_as("SELECT id, min_reader FROM schema_compat")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(compat, vec![(1, MIN_READER.to_owned())]);
}

/// Inserts an operation row by raw SQL with the given status name.
async fn raw_operation(store: &Store, status: &str) -> OperationId {
    let id = OperationId::new();
    sqlx::query(
        "INSERT INTO operations (id, kind, goal, status, budgets, pinned, created_at, updated_at) \
         VALUES (?, 'investigate', 'a goal', ?, ?, '[]', 0, 0)",
    )
    .bind(id.to_string())
    .bind(status)
    .bind(serde_json::to_string(&Budgets::default()).unwrap())
    .execute(store.pool_for_tests())
    .await
    .unwrap();
    id
}

/// Inserts a note step row by raw SQL.
async fn raw_step(store: &Store, operation: OperationId, ordinal: i64) -> StepId {
    let id = StepId::new();
    let intent = StepIntent::Note {
        text: "a note".to_owned(),
        author: Actor::System("test".to_owned()),
    };
    sqlx::query(
        "INSERT INTO steps (id, operation_id, ordinal, kind, phase, cycle, intent, created_at) \
         VALUES (?, ?, ?, 'note', 'phase', 0, ?, 0)",
    )
    .bind(id.to_string())
    .bind(operation.to_string())
    .bind(ordinal)
    .bind(serde_json::to_string(&intent).unwrap())
    .execute(store.pool_for_tests())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn step_intent_columns_cannot_be_updated() {
    let fresh = Fresh::new().await;
    let operation = raw_operation(&fresh.store, "queued").await;
    let step = raw_step(&fresh.store, operation, 0).await;

    for assignment in [
        "id = id",
        "operation_id = operation_id",
        "ordinal = 5",
        "kind = 'plan'",
        "phase = 'other'",
        "parent_step_id = NULL",
        "cycle = 1",
        "intent = '{}'",
        "created_at = 9",
    ] {
        let sql = format!("UPDATE steps SET {assignment} WHERE id = ?");
        let outcome = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(step.to_string())
            .execute(fresh.store.pool_for_tests())
            .await;
        let message = outcome.expect_err(assignment).to_string();
        assert!(message.contains("immutable"), "{assignment}: {message}");
    }
    // Progress columns stay writable.
    sqlx::query("UPDATE steps SET status = 'running', started_at = 1, summary = 's' WHERE id = ?")
        .bind(step.to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
}

#[tokio::test]
async fn step_edge_trigger_refuses_a_backwards_edge_written_by_raw_sql() {
    let fresh = Fresh::new().await;
    let one = raw_operation(&fresh.store, "queued").await;
    let two = raw_operation(&fresh.store, "queued").await;
    let first = raw_step(&fresh.store, one, 0).await;
    let second = raw_step(&fresh.store, one, 1).await;
    let elsewhere = raw_step(&fresh.store, two, 0).await;

    let insert = |operation: OperationId, from: StepId, to: StepId| {
        let pool = fresh.store.pool_for_tests().clone();
        async move {
            sqlx::query(
                "INSERT INTO step_edges (operation_id, from_step, to_step, kind) \
                 VALUES (?, ?, ?, 'depends_on')",
            )
            .bind(operation.to_string())
            .bind(from.to_string())
            .bind(to.to_string())
            .execute(&pool)
            .await
        }
    };
    for (operation, from, to) in [
        (one, first, second),
        (one, second, elsewhere),
        (two, second, first),
    ] {
        let message = insert(operation, from, to).await.unwrap_err().to_string();
        assert!(message.contains("earlier step"), "{message}");
    }
    insert(one, second, first).await.unwrap();
}

#[tokio::test]
async fn unknown_stored_operation_status_is_a_store_error() {
    let fresh = Fresh::new().await;
    let id = raw_operation(&fresh.store, "exploded").await;

    let got = OperationRepo::get(&fresh.store, id).await;
    assert!(
        matches!(&got, Err(StoreError::Backend(message)) if message.contains("exploded")),
        "{got:?}"
    );
    let listed = fresh.store.with_status(&["exploded"], 10).await;
    assert!(matches!(listed, Err(StoreError::Backend(_))), "{listed:?}");

    // A paused row without its reason cannot be read either.
    let paused = raw_operation(&fresh.store, "paused").await;
    let got = OperationRepo::get(&fresh.store, paused).await;
    assert!(
        matches!(&got, Err(StoreError::Backend(message)) if message.contains("paused")),
        "{got:?}"
    );

    // An unknown step status fails the snapshot and names the value.
    let queued = raw_operation(&fresh.store, "queued").await;
    let step = raw_step(&fresh.store, queued, 0).await;
    sqlx::query("UPDATE steps SET status = 'bogus' WHERE id = ?")
        .bind(step.to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    let loaded = fresh.store.load(queued).await;
    assert!(
        matches!(&loaded, Err(StoreError::Backend(message)) if message.contains("bogus")),
        "{loaded:?}"
    );
}
