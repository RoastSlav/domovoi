//! What SQLite stores for policy beyond the shared contracts: immutability triggers and
//! durability across a reopen.

use domovoi_core::policy_store::{DraftGuard, PolicyWrite, StoredRule};
use domovoi_core::ports::{PolicyRepo, StateWrite, Transaction, UnitOfWork};
use domovoi_core::{Actor, PolicyVersionId};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{T0, at};

use crate::common;

const HASH: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn rule() -> StoredRule {
    StoredRule {
        rule_id: "a".to_owned(),
        ordinal: 0,
        enabled: true,
        shadow: false,
        locked: false,
        body: "{\"v\":1}".to_owned(),
    }
}

async fn publish_one(store: &Store) -> PolicyVersionId {
    let id = PolicyVersionId::new();
    let actor = Actor::System("test".to_owned());
    let save = PolicyWrite::SaveDraft {
        id,
        expected: DraftGuard::Absent,
        based_on: None,
        rules: vec![rule()],
        rules_hash: HASH.to_owned(),
        actor: actor.clone(),
        at: at(10),
    };
    let publish = PolicyWrite::Publish {
        id,
        expected_revision: 1,
        number: 1,
        acknowledgements: vec![],
        actor,
        at: at(20),
    };
    for write in [save, publish] {
        store
            .commit(Transaction::new().write(StateWrite::Policy(write)))
            .await
            .unwrap();
    }
    id
}

#[tokio::test]
async fn published_rules_cannot_be_rewritten() {
    let fresh = common::Fresh::new().await;
    let id = publish_one(&fresh.store).await;
    let update = sqlx::query("UPDATE policy_rules SET body = 'x' WHERE version_id = ?")
        .bind(id.to_string())
        .execute(fresh.store.pool_for_tests())
        .await;
    assert!(
        update
            .unwrap_err()
            .to_string()
            .contains("a published policy version is immutable")
    );
    let delete = sqlx::query("DELETE FROM policy_rules WHERE version_id = ?")
        .bind(id.to_string())
        .execute(fresh.store.pool_for_tests())
        .await;
    assert!(
        delete
            .unwrap_err()
            .to_string()
            .contains("a published policy version is immutable")
    );
}

#[tokio::test]
async fn policy_write_survives_reopen() {
    let fresh = common::Fresh::new().await;
    let id = publish_one(&fresh.store).await;
    fresh.store.close().await;
    let path = fresh.path();
    let reopened = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    let active = PolicyRepo::active(&reopened).await.unwrap().unwrap();
    assert_eq!((active.id, active.number), (id, Some(1)));
    assert_eq!(
        PolicyRepo::rules(&reopened, id).await.unwrap(),
        vec![rule()]
    );
}
