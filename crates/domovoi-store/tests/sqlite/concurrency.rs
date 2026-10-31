//! Concurrent writers: atomic token consumption, and writers that wait instead of failing.

use std::collections::BTreeSet;

use domovoi_core::ports::{AuditEvent, AuditLog, EnrollmentToken, HostRepo, StoreError};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{T0, at, host};

use crate::common::Fresh;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enrollment_token_consumption_is_atomic_under_concurrency() {
    let fresh = Fresh::new().await;
    let nas = host("nas").unwrap();
    fresh.store.upsert_host(&nas).await.unwrap();
    for round in 0..20u8 {
        let token = EnrollmentToken {
            host: nas.id,
            token_hash: [round; 32],
            expires_at: at(3600),
        };
        fresh.store.create_enrollment_token(&token).await.unwrap();
        // A second store on the same file races too, as a second process would.
        let other = Store::open(&fresh.path(), StoreOptions::beside(&fresh.path(), T0))
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            tokio::spawn({
                let store = fresh.store.clone();
                async move { store.consume_enrollment_token(&[round; 32], at(10)).await }
            }),
            tokio::spawn(async move { other.consume_enrollment_token(&[round; 32], at(10)).await }),
        );
        let results = [a.unwrap(), b.unwrap()];
        let won = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(won, 1, "round {round}: {results:?}");
        assert!(results.contains(&Err(StoreError::NotFound)));
        assert!(results.contains(&Ok(nas.id)));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_database_retries_instead_of_failing() {
    let fresh = Fresh::new().await;
    // Two stores on one file: separate writer connections contend for the SQLite lock.
    let second = Store::open(&fresh.path(), StoreOptions::beside(&fresh.path(), T0))
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for writer in 0..8 {
        let store = if writer % 2 == 0 {
            fresh.store.clone()
        } else {
            second.clone()
        };
        tasks.push(tokio::spawn(async move {
            let mut seqs = Vec::new();
            for n in 0..25 {
                let event = AuditEvent {
                    at: at(n),
                    actor: format!("system:writer{writer}"),
                    kind: "test.write".to_owned(),
                    subject: format!("n:{n}"),
                    detail: serde_json::Value::Null,
                };
                seqs.push(store.append(&event).await);
            }
            seqs
        }));
    }
    let mut all = BTreeSet::new();
    for task in tasks {
        for seq in task.await.unwrap() {
            assert!(all.insert(seq.unwrap()), "sequence numbers are unique");
        }
    }
    assert_eq!(all.len(), 200);
    assert_eq!(all.iter().next(), Some(&1));
    assert_eq!(all.iter().last(), Some(&200));
}
