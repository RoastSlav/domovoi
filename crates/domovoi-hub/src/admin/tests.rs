use std::sync::Arc;

use domovoi_config::MapEnv;
use domovoi_core::audit_chain::{ChainStatus, VerifyScope};
use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, HostRepo, Page, UserRepo};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::FakeClock;
use jiff::SignedDuration;

use super::{AdminError, OfflineAdmin};
use crate::auth::password;
use crate::runtime::HubLock;
use crate::runtime::adapters::verify_audit_on_start;

fn config(dir: &std::path::Path) -> domovoi_config::HubConfig {
    let text = format!(
        r#"
[hub]
name = "adminhub"
data_dir = '{}'

[agent_server]
public_url = "wss://adminhub:7421"
"#,
        dir.display()
    );
    domovoi_config::parse(&text, &MapEnv::new()).unwrap().0
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new("2026-11-01T12:00:00Z".parse().unwrap()))
}

#[tokio::test]
async fn refuses_while_a_hub_holds_the_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let _hub = HubLock::acquire(dir.path()).unwrap();
    let err = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .err()
        .unwrap();
    assert!(matches!(err, AdminError::HubRunning(_)), "{err}");
    assert!(err.to_string().contains("web UI"));
}

#[tokio::test]
async fn token_then_revoke_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    let invite = admin
        .create_token("web-1", SignedDuration::from_mins(30))
        .await
        .unwrap();
    assert!(
        invite.command.contains("--fingerprint"),
        "{}",
        invite.command
    );
    assert_eq!(
        invite.expires_at,
        "2026-11-01T12:30:00Z".parse::<jiff::Timestamp>().unwrap()
    );
    assert!(
        admin
            .create_token("web-1", SignedDuration::ZERO)
            .await
            .is_err()
    );
    let rows = admin.agents().await.unwrap();
    let names: Vec<_> = rows.iter().map(|r| r.host.as_str()).collect();
    assert_eq!(names, ["adminhub", "web-1"]);
    assert!(
        rows.iter().all(|r| r.version.is_none()),
        "nobody enrolled yet"
    );
    assert!(!admin.revoke("web-1").await.unwrap(), "no agent to revoke");
    assert!(matches!(
        admin.revoke("nope").await,
        Err(AdminError::Invalid(_))
    ));
    admin.close().await;
}

#[tokio::test]
async fn token_create_and_agent_revoke_from_the_command_line_leave_audit_rows() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    let invite = admin
        .create_token("web-1", SignedDuration::from_mins(30))
        .await
        .unwrap();
    let host = admin
        .store
        .get_host_by_name(&domovoi_core::HostName::parse("web-1").unwrap())
        .await
        .unwrap()
        .unwrap();
    admin
        .store
        .put_agent(&domovoi_core::AgentIdentity {
            agent: domovoi_core::AgentId::new(),
            host: host.id,
            public_key: [0; 32],
            version: "0.0.1".to_owned(),
            enrolled_at: "2026-11-01T12:00:00Z".parse().unwrap(),
            revoked_at: None,
        })
        .await
        .unwrap();
    assert!(admin.revoke("web-1").await.unwrap());
    let rows = AuditLog::list(&*admin.store, &AuditFilter::default(), &Page::first(50))
        .await
        .unwrap()
        .items;
    let find = |kind: &str| rows.iter().find(|row| row.kind == kind);
    let invited = find("host.invite_created").expect("an invite row");
    assert_eq!(invited.actor, "system:cli");
    assert_eq!(invited.subject, "host:web-1");
    let revoked = find("host.agent_revoked").expect("a revoke row");
    assert_eq!(revoked.actor, "system:cli");
    assert_eq!(revoked.subject, "host:web-1");
    assert!(
        rows.iter()
            .all(|row| !row.detail.to_string().contains(invite.token.expose())),
        "no row holds the token"
    );
    admin.close().await;
}

#[tokio::test]
async fn set_password_creates_the_admin_then_updates_it() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    assert!(matches!(
        admin.set_password(None, "short").await,
        Err(AdminError::Invalid(_))
    ));
    assert_eq!(
        admin
            .set_password(None, "correct horse battery")
            .await
            .unwrap()
            .username,
        "admin"
    );
    assert_eq!(
        admin
            .set_password(None, "another long password")
            .await
            .unwrap()
            .username,
        "admin"
    );
    let user = admin.store.first_user().await.unwrap().unwrap();
    assert!(password::verify(
        "another long password",
        &user.password_hash
    ));
    assert!(matches!(
        admin
            .set_password(Some("ghost"), "another long password")
            .await,
        Err(AdminError::Invalid(_))
    ));
    // Each success left an audit row, and none of them holds a password.
    let filter = AuditFilter {
        kind: Some("hub.password_set".to_owned()),
        ..AuditFilter::default()
    };
    let rows = AuditLog::list(&*admin.store, &filter, &Page::first(10))
        .await
        .unwrap()
        .items;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| !row.detail.to_string().contains("horse"))
    );
    admin.close().await;
}

#[tokio::test]
async fn digest_without_a_notifier_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    assert!(matches!(
        admin.send_digest().await,
        Err(AdminError::Delivery(_))
    ));
    admin.close().await;
}

fn event(kind: &str) -> AuditEvent {
    AuditEvent {
        at: "2026-11-01T12:00:00Z".parse().unwrap(),
        actor: "system:test".to_owned(),
        kind: kind.to_owned(),
        subject: "subject".to_owned(),
        detail: serde_json::Value::Null,
    }
}

async fn store_at(dir: &std::path::Path) -> Store {
    let path = dir.join("hub.db");
    Store::open(
        &path,
        StoreOptions::beside(&path, "2026-11-01T12:00:00Z".parse().unwrap()),
    )
    .await
    .unwrap()
}

async fn break_the_chain(store: &Store) {
    for kind in ["test.1", "test.2", "test.3"] {
        store.append(&event(kind)).await.unwrap();
    }
    let pool = store.pool_for_tests();
    sqlx::query("DROP TRIGGER audit_log_no_update")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE audit_log SET detail = '{\"edited\":true}' WHERE seq = 2")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN \
         SELECT RAISE(ABORT, 'audit_log is append-only'); END",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn broken_rows(store: &Store) -> usize {
    let filter = AuditFilter {
        kind: Some("audit.chain_broken".to_owned()),
        ..AuditFilter::default()
    };
    AuditLog::list(store, &filter, &Page::first(10))
        .await
        .unwrap()
        .items
        .len()
}

#[tokio::test]
async fn startup_verification_records_a_broken_chain_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(dir.path()).await;
    break_the_chain(&store).await;

    let clock = FakeClock::new("2026-11-01T12:00:00Z".parse().unwrap());
    verify_audit_on_start(&store, &clock).await;
    verify_audit_on_start(&store, &clock).await;

    assert_eq!(broken_rows(&store).await, 1);
}

#[tokio::test]
async fn startup_verification_is_silent_on_an_intact_chain() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(dir.path()).await;
    for kind in ["test.1", "test.2"] {
        store.append(&event(kind)).await.unwrap();
    }

    let clock = FakeClock::new("2026-11-01T12:00:00Z".parse().unwrap());
    verify_audit_on_start(&store, &clock).await;
    verify_audit_on_start(&store, &clock).await;

    assert_eq!(broken_rows(&store).await, 0);
    assert!(matches!(
        store.verify_audit(VerifyScope::Full).await.unwrap().chained,
        ChainStatus::Intact { checked: 2, .. }
    ));
}

#[tokio::test]
async fn admin_verify_reports_an_empty_chain_on_a_fresh_database() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    let report = admin.verify_audit(VerifyScope::Full).await.unwrap();
    assert_eq!(report.chained, ChainStatus::Empty);
    assert_eq!(admin.audit_head().await.unwrap(), None);
    admin.close().await;
}

#[tokio::test]
async fn a_restore_leaves_an_audit_row_in_the_restored_history() {
    let dir = tempfile::tempdir().unwrap();
    let backups = tempfile::tempdir().unwrap();
    let now: jiff::Timestamp = "2026-11-01T12:00:00Z".parse().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    let file = admin.store.backup_to(backups.path(), now).await.unwrap();
    admin.close().await;

    super::restore(dir.path(), &file, now).await.unwrap();

    let store = Store::open(
        &dir.path().join("hub.db"),
        StoreOptions::beside(&dir.path().join("hub.db"), now),
    )
    .await
    .unwrap();
    let filter = AuditFilter {
        kind: Some("backup.restored".to_owned()),
        ..AuditFilter::default()
    };
    let rows = AuditLog::list(&store, &filter, &Page::first(10))
        .await
        .unwrap()
        .items;
    assert_eq!(rows.len(), 1);
    store.close().await;
}

#[tokio::test]
async fn set_password_ends_every_session_of_the_account_and_says_how_many() {
    use domovoi_core::Session;
    use domovoi_core::ports::SessionRepo;

    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    admin
        .set_password(None, "correct horse battery")
        .await
        .unwrap();
    let user = admin.store.first_user().await.unwrap().unwrap();
    let now = admin.clock.now();
    let mut hashes = Vec::new();
    for n in 0..2u8 {
        let session = Session {
            token_hash: [n + 1; 32],
            user: user.id,
            csrf_hash: [9; 32],
            created_at: now,
            expires_at: now.checked_add(SignedDuration::from_hours(24)).unwrap(),
        };
        SessionRepo::create(&*admin.store, &session).await.unwrap();
        hashes.push(session.token_hash);
    }
    let done = admin
        .set_password(None, "another long password")
        .await
        .unwrap();
    assert_eq!(done.sessions_ended, 2);
    for hash in &hashes {
        assert!(
            SessionRepo::get(&*admin.store, hash, now)
                .await
                .unwrap()
                .is_none(),
            "an old session survived the password change"
        );
    }
    let filter = AuditFilter {
        kind: Some("hub.password_set".to_owned()),
        ..AuditFilter::default()
    };
    let rows = AuditLog::list(&*admin.store, &filter, &Page::first(10))
        .await
        .unwrap()
        .items;
    assert_eq!(rows[0].detail["sessions_ended"], 2, "newest row first");
    admin.close().await;
}
