//! The schema a fresh database ends up with is the schema the code expects, and it does not
//! drift unnoticed.

use crate::common::Fresh;

/// Every table with its columns, then the indexes, triggers and views, one line each.
async fn describe(store: &domovoi_store::Store) -> String {
    let pool = store.pool_for_tests();
    let objects: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT type, name, tbl_name FROM sqlite_master \
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
           AND NOT (type = 'table' AND name LIKE '%\\_fts\\_%' ESCAPE '\\') \
           AND sql IS NOT NULL \
         ORDER BY type, name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut lines = Vec::new();
    for (kind, name, table) in objects {
        match kind.as_str() {
            "table" => {
                let columns: Vec<(String, String, i64, i64)> =
                    sqlx::query_as("SELECT name, type, \"notnull\", pk FROM pragma_table_info(?)")
                        .bind(&name)
                        .fetch_all(pool)
                        .await
                        .unwrap();
                let columns: Vec<String> = columns
                    .into_iter()
                    .map(|(column, ty, not_null, pk)| {
                        let mut text = format!("{column} {ty}");
                        if pk > 0 {
                            text.push_str(" pk");
                        }
                        if not_null > 0 {
                            text.push_str(" not null");
                        }
                        text
                    })
                    .collect();
                lines.push(format!("table {name}: {}", columns.join(", ")));
            }
            "view" => lines.push(format!("view {name}")),
            other => lines.push(format!("{other} {name} on {table}")),
        }
    }
    lines.join("\n") + "\n"
}

#[tokio::test]
async fn baseline_migration_matches_the_expected_schema() {
    let fresh = Fresh::new().await;
    let actual = describe(&fresh.store).await;
    assert_eq!(
        actual,
        include_str!("baseline_schema.txt"),
        "the schema changed: if that is intended, update baseline_schema.txt from this output"
    );
}

#[tokio::test]
async fn a_fresh_database_has_no_table_for_the_removed_action_rows() {
    let fresh = Fresh::new().await;
    let found: Option<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE name = 'actions'")
            .fetch_optional(fresh.store.pool_for_tests())
            .await
            .unwrap();
    assert!(
        found.is_none(),
        "the `actions` table is not part of the schema"
    );
}

#[tokio::test]
async fn a_fresh_database_is_created_by_exactly_one_migration() {
    let fresh = Fresh::new().await;
    let versions: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _sqlx_migrations")
        .fetch_all(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(versions, vec![(1,)]);
}
