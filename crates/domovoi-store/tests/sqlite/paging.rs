//! Keyset pagination with equal timestamps, and millisecond timestamps.

use std::collections::BTreeSet;

use domovoi_core::ports::{IncidentFilter, IncidentRepo, Page, UserRepo};
use domovoi_core::{HostId, IncidentId, User, UserId};
use domovoi_testkit::build::{T0, incident};
use jiff::{SignedDuration, Timestamp};

use crate::common::Fresh;

#[tokio::test]
async fn pagination_is_stable_with_equal_timestamps() {
    let fresh = Fresh::new().await;
    let host = HostId::new();
    let mut ids = Vec::new();
    // Every incident opens at T0, so only the id orders them.
    for n in 0..7 {
        let item = incident(host, &format!("test.check_{n}"), None).unwrap();
        IncidentRepo::insert(&fresh.store, &item).await.unwrap();
        ids.push(item.id());
    }
    ids.sort();
    ids.reverse();

    let filter = IncidentFilter::default();
    let mut page = Page::first(2);
    let mut seen = Vec::new();
    loop {
        let got = IncidentRepo::list(&fresh.store, &filter, &page)
            .await
            .unwrap();
        seen.extend(got.items.iter().map(|i| i.id()));
        // A newer incident arriving mid-listing does not shift the pages.
        let late = incident(host, &format!("late.check_{}", seen.len()), None).unwrap();
        IncidentRepo::insert(&fresh.store, &late).await.unwrap();
        match got.next_cursor {
            Some(cursor) => {
                page = Page {
                    cursor: Some(cursor),
                    limit: 2,
                }
            }
            None => break,
        }
    }
    let late: BTreeSet<IncidentId> = seen
        .iter()
        .copied()
        .filter(|id| !ids.contains(id))
        .collect();
    let original: Vec<IncidentId> = seen.into_iter().filter(|id| ids.contains(id)).collect();
    assert_eq!(original, ids, "no gaps, no repeats, id descending");
    // Late arrivals share the timestamp but have larger ids, so they sort before every
    // cursor already handed out and never show up on a later page.
    assert!(late.is_empty(), "{late:?}");

    // A cursor that was not issued by the store is rejected.
    let bad = Page {
        cursor: Some("not-a-cursor".to_owned()),
        limit: 2,
    };
    assert!(
        IncidentRepo::list(&fresh.store, &filter, &bad)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn timestamps_round_trip_with_millisecond_precision() {
    let fresh = Fresh::new().await;
    let precise = T0 + SignedDuration::from_millis(123) + SignedDuration::from_micros(456);
    let user = User {
        id: UserId::new(),
        username: "alice".to_owned(),
        password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_owned(),
        created_at: precise,
    };
    fresh.store.create(&user).await.unwrap();
    let back = UserRepo::get(&fresh.store, user.id).await.unwrap().unwrap();
    let expected = Timestamp::from_millisecond(T0.as_millisecond() + 123).unwrap();
    assert_eq!(
        back.created_at, expected,
        "kept to the millisecond, below dropped"
    );

    let before_epoch = Timestamp::from_millisecond(-1_234).unwrap();
    let old = User {
        id: UserId::new(),
        username: "bob".to_owned(),
        created_at: before_epoch,
        ..user
    };
    fresh.store.create(&old).await.unwrap();
    assert_eq!(
        UserRepo::get(&fresh.store, old.id)
            .await
            .unwrap()
            .unwrap()
            .created_at,
        before_epoch
    );
}
