use std::sync::Arc;

use domovoi_config::MapEnv;
use domovoi_core::audit_chain::{ChainStatus, LegacyStatus, VerifyScope};
use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page, UserRepo};
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
            .unwrap(),
        "admin"
    );
    assert_eq!(
        admin
            .set_password(None, "another long password")
            .await
            .unwrap(),
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
async fn admin_verify_reports_both_parts() {
    let dir = tempfile::tempdir().unwrap();
    let admin = OfflineAdmin::open(config(dir.path()), clock())
        .await
        .unwrap();
    let report = admin.verify_audit(VerifyScope::Full).await.unwrap();
    assert!(matches!(report.legacy, LegacyStatus::Verified { count: 0 }));
    assert_eq!(report.chained, ChainStatus::Empty);
    assert_eq!(admin.audit_head().await.unwrap(), None);
    admin.close().await;
}
