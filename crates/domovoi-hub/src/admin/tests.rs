use std::sync::Arc;

use domovoi_config::MapEnv;
use domovoi_core::ports::UserRepo;
use domovoi_testkit::FakeClock;
use jiff::SignedDuration;

use super::{AdminError, OfflineAdmin};
use crate::auth::password;
use crate::runtime::HubLock;

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
