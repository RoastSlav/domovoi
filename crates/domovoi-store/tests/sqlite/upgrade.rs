//! Upgrade from a database written by the previous schema.
//!
//! `tests/fixtures/n1-schema5.db` was produced from the code before this change, with
//! migrations 0001 to 0005 only, and cannot be regenerated: the generator has been deleted
//! so the fixture stays a fixed picture of the previous release.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use domovoi_core::ports::{AdvisoryRepo, HostRepo, IncidentRepo, NoteRepo};
use domovoi_core::{Incident, IncidentId};
use domovoi_store::{MIGRATOR, MIN_READER, Store, StoreOptions};
use domovoi_testkit::build::at;
use sqlx::ConnectOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("n1-schema5.db")
}

/// Copies the fixture into a temporary database directory and returns the new path.
fn copy_fixture(dir: &Path) -> PathBuf {
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).unwrap();
    let path = dbdir.join("domovoi.db");
    std::fs::copy(fixture(), &path).unwrap();
    path
}

async fn raw(path: &Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .unwrap()
}

#[tokio::test]
async fn upgrade_from_previous_schema_keeps_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let backup_dir = dir.path().join("db").join("backups");
    let store = Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap();

    // 1. The pre-migration backup is the previous schema, without the new tables.
    let backup = backup_dir.join(domovoi_store::pre_migrate_name(at(1000)));
    assert!(backup.exists(), "{} was not written", backup.display());
    let mut old = raw(&backup).await;
    for table in ["journal", "effects", "schema_compat"] {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_optional(&mut old)
                .await
                .unwrap();
        assert!(found.is_none(), "the backup has no {table}");
    }
    sqlx::Connection::close(old).await.unwrap();

    // 2. Every row survives.
    let mut original = raw(&fixture()).await;
    let mut expected = Vec::new();
    for table in [
        "hosts",
        "incidents",
        "signals",
        "incident_events",
        "actions",
        "audit_log",
        "notes",
        "settings",
    ] {
        let sql = format!("SELECT count(*) FROM {table}");
        let count: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut original)
            .await
            .unwrap();
        expected.push((table, count.0));
    }
    sqlx::Connection::close(original).await.unwrap();
    let mut actual = Vec::new();
    for (table, _) in &expected {
        let sql = format!("SELECT count(*) FROM {table}");
        let count: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_one(store.pool_for_tests())
            .await
            .unwrap();
        actual.push((*table, count.0));
    }
    assert_eq!(actual, expected);

    // 3. Versions start at 1 and every incident still loads through its repository.
    let incidents: Vec<(String, i64)> = sqlx::query_as("SELECT id, version FROM incidents")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(incidents.len(), 3);
    for (id, version) in &incidents {
        assert_eq!(*version, 1);
        let id: IncidentId = id.parse().unwrap();
        assert!(IncidentRepo::get(&store, id).await.unwrap().is_some());
    }
    let body: (String,) = sqlx::query_as("SELECT body FROM incidents LIMIT 1")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    let _: Incident = serde_json::from_str(&body.0).unwrap();
    let note_versions: Vec<(i64,)> = sqlx::query_as("SELECT version FROM notes")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(note_versions, vec![(1,), (1,)]);

    // 4. The new tables start empty and the compatibility row is written.
    let journal: (i64,) = sqlx::query_as("SELECT count(*) FROM journal")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    let effects: (i64,) = sqlx::query_as("SELECT count(*) FROM effects")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!((journal.0, effects.0), (0, 0));
    let compat: Vec<(i64, String)> = sqlx::query_as("SELECT id, min_reader FROM schema_compat")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(compat, vec![(1, MIN_READER.to_owned())]);

    // 5. The audit triggers are still there and still refuse an update.
    let triggers: BTreeSet<String> = sqlx::query_as::<_, (String,)>(
        "SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'audit_log_no_%'",
    )
    .fetch_all(store.pool_for_tests())
    .await
    .unwrap()
    .into_iter()
    .map(|(name,)| name)
    .collect();
    assert_eq!(
        triggers,
        BTreeSet::from([
            "audit_log_no_delete".to_owned(),
            "audit_log_no_replace".to_owned(),
            "audit_log_no_update".to_owned(),
        ])
    );
    assert!(
        sqlx::query("UPDATE audit_log SET actor = actor WHERE seq = 1")
            .execute(store.pool_for_tests())
            .await
            .is_err()
    );

    // 6. Full-text search still finds the titled rows.
    assert!(
        !IncidentRepo::search(&store, "disk", 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !NoteRepo::search(&store, "scrub", None, 10)
            .await
            .unwrap()
            .is_empty()
    );

    // 7. Every embedded migration is recorded as successful.
    let applied: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(store.pool_for_tests())
            .await
            .unwrap();
    let expected_versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(
        applied.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        expected_versions
    );
    assert!(applied.iter().all(|(_, success)| *success));
}

/// An agent row written before the report columns existed still reads back, with no report
/// until it next authenticates.
#[tokio::test]
async fn migration_keeps_existing_agents() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let mut conn = raw(&path).await;
    let host_id = "0190a0a0-0000-7000-8000-0000000009a1";
    let agent_id = "0190a0a0-0000-7000-8000-0000000009a2";
    sqlx::query(
        "INSERT INTO hosts (id, name, kind, labels, created_at) VALUES (?, 'legacy-box', 'Remote', '{}', ?)",
    )
    .bind(host_id)
    .bind(at(0).as_millisecond())
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO agents (agent_id, host_id, public_key, version, enrolled_at) \
         VALUES (?, ?, ?, '0.0.1', ?)",
    )
    .bind(agent_id)
    .bind(host_id)
    .bind([7u8; 32].to_vec())
    .bind(at(0).as_millisecond())
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let store = Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap();
    let agent = HostRepo::get_agent(&store, agent_id.parse().unwrap())
        .await
        .unwrap()
        .expect("the existing agent survived the migration");
    assert_eq!(agent.version, "0.0.1");
    assert_eq!(agent.host.to_string(), host_id);
    assert_eq!(
        HostRepo::agent_report(&store, agent.agent).await.unwrap(),
        None,
        "an old row reports nothing until the agent authenticates"
    );
}

#[tokio::test]
async fn advisories_rows_survive_kind_check_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let mut conn = raw(&path).await;
    sqlx::query(
        "INSERT INTO advisories (id, kind, host_id, title, detail, created_at, resolved_at) \
         VALUES (?, 'heartbeat_missing', NULL, 'Agent gone', 'No heartbeat', ?, NULL)",
    )
    .bind("0190a0a0-0000-7000-8000-000000000901")
    .bind(at(0).as_millisecond())
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::Connection::close(conn).await.unwrap();

    let store = Store::open(&path, StoreOptions::beside(&path, at(1000)))
        .await
        .unwrap();
    let row: (String, String) = sqlx::query_as("SELECT kind, title FROM advisories WHERE id = ?")
        .bind("0190a0a0-0000-7000-8000-000000000901")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(row.0, "heartbeat_missing");
    assert_eq!(row.1, "Agent gone");
}

#[tokio::test]
async fn advisory_with_unknown_kind_is_a_store_error() {
    let fresh = crate::common::Fresh::new().await;
    sqlx::query(
        "INSERT INTO advisories (id, kind, host_id, title, detail, created_at, resolved_at) \
         VALUES (?, 'bogus', NULL, 'Unknown', 'Unknown kind', ?, NULL)",
    )
    .bind("0190a0a0-0000-7000-8000-000000000902")
    .bind(at(0).as_millisecond())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
    assert!(fresh.store.list_open().await.is_err());
}
