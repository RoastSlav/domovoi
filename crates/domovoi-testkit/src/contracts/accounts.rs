//! Contracts for [`SettingsRepo`], [`UserRepo`] and [`SessionRepo`].

use domovoi_core::ports::{SessionRepo, SettingsRepo, StoreError, UserRepo};
use domovoi_core::{Session, User, UserId};
use serde_json::json;

use super::ContractResult;
use crate::build::{T0, at};

/// Exercises get and set on an empty `repo`.
pub async fn settings_repo_contract(repo: &dyn SettingsRepo) -> ContractResult {
    assert_eq!(repo.get("triage.enabled").await?, None);
    repo.set("triage.enabled", &json!(true)).await?;
    repo.set("digest.time", &json!({ "hour": 8 })).await?;
    assert_eq!(repo.get("triage.enabled").await?, Some(json!(true)));
    repo.set("triage.enabled", &json!(false)).await?;
    assert_eq!(
        repo.get("triage.enabled").await?,
        Some(json!(false)),
        "set replaces"
    );
    assert_eq!(repo.get("digest.time").await?, Some(json!({ "hour": 8 })));
    Ok(())
}

fn user(username: &str) -> User {
    User {
        id: UserId::new(),
        username: username.to_owned(),
        password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_owned(),
        created_at: T0,
    }
}

/// Exercises account creation, lookup and password changes on an empty `repo`.
pub async fn user_repo_contract(repo: &dyn UserRepo) -> ContractResult {
    assert_eq!(repo.count().await?, 0);
    let admin = user("admin");
    repo.create(&admin).await?;
    assert_eq!(repo.count().await?, 1);
    assert_eq!(repo.get(admin.id).await?, Some(admin.clone()));
    assert_eq!(repo.get_by_username("admin").await?, Some(admin.clone()));
    assert_eq!(
        repo.get_by_username("Admin").await?,
        None,
        "usernames are case-sensitive"
    );

    assert!(matches!(
        repo.create(&admin).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(
        matches!(
            repo.create(&user("admin")).await,
            Err(StoreError::Conflict(_))
        ),
        "usernames are unique"
    );
    assert_eq!(repo.count().await?, 1);

    repo.set_password_hash(admin.id, "$argon2id$new").await?;
    let changed = repo.get(admin.id).await?.map(|u| u.password_hash);
    assert_eq!(changed.as_deref(), Some("$argon2id$new"));
    assert_eq!(
        repo.set_password_hash(UserId::new(), "x").await,
        Err(StoreError::NotFound)
    );
    Ok(())
}

/// Exercises session lookup, expiry and deletion on an empty `repo`.
pub async fn session_repo_contract(repo: &dyn SessionRepo) -> ContractResult {
    let (alice, bob) = (UserId::new(), UserId::new());
    let session = |hash: u8, user, expires| Session {
        token_hash: [hash; 32],
        user,
        csrf_hash: [hash.wrapping_add(100); 32],
        created_at: T0,
        expires_at: at(expires),
    };
    let first = session(1, alice, 100);
    repo.create(&first).await?;
    repo.create(&session(2, alice, 50)).await?;
    repo.create(&session(3, bob, 200)).await?;
    assert!(matches!(
        repo.create(&first).await,
        Err(StoreError::Conflict(_))
    ));

    assert_eq!(repo.get(&[1; 32], at(99)).await?, Some(first.clone()));
    assert_eq!(
        repo.get(&[1; 32], at(100)).await?,
        None,
        "expired at expires_at"
    );
    assert_eq!(repo.get(&[9; 32], T0).await?, None);

    assert_eq!(repo.purge_expired(at(50)).await?, 1);
    assert_eq!(
        repo.get(&[2; 32], at(0)).await?,
        None,
        "purged sessions are gone"
    );
    assert_eq!(repo.delete_for_user(alice).await?, 1);
    assert_eq!(repo.get(&[1; 32], at(0)).await?, None);

    repo.delete(&[3; 32]).await?;
    repo.delete(&[3; 32]).await?;
    assert_eq!(repo.get(&[3; 32], at(0)).await?, None);
    Ok(())
}
