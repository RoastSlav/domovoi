//! Retention deletes old signals, timeline entries, conversations, notification state,
//! cache entries and resolved advisories, and never incidents or the audit log.

use domovoi_core::ports::{
    AdvisoryRepo, AuditEvent, AuditFilter, AuditLog, IncidentRepo, IncidentTimelineEntry,
    NotifyStateRepo, Page, SignalRepo, TriageCacheRepo,
};
use domovoi_core::triage::{CLEAR_KIND, FOLLOW_UP_KIND};
use domovoi_core::{
    Advisory, AdvisoryId, AdvisoryKind, HeldItem, HostId, NotifyState, Severity, TriageReport,
    Verdict,
};
use domovoi_store::{FTS_CLEANUP, PURGED_TABLES, PurgeReport, RetentionPolicy};
use domovoi_testkit::build::{T0, incident, open_incident, resolved_incident, signal};
use jiff::{SignedDuration, Timestamp};

use crate::common::Fresh;

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * n)
}

fn policy_days(n: i64) -> RetentionPolicy {
    RetentionPolicy {
        signals: days(n),
        timeline: days(n),
        conversations: days(n),
        notifications: days(n),
        triage: days(n),
        resolved_advisories: days(n),
    }
}

fn report() -> TriageReport {
    TriageReport {
        headline: "Disk filling".to_owned(),
        summary: "Logs grow.".to_owned(),
        cause: "Rotation is off.".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::Monitor,
        next_step: "Rotate logs.".to_owned(),
        manual_steps: Vec::new(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

async fn old_signal(
    store: &domovoi_store::Store,
    host: HostId,
    incident: domovoi_core::IncidentId,
    check: &str,
    at: Timestamp,
) {
    let s = signal(host, check, None).unwrap();
    let s = domovoi_core::Signal::firing(
        s.source().clone(),
        host,
        s.check().clone(),
        None,
        s.severity(),
        s.summary(),
        at,
    );
    SignalRepo::insert(store, &s, Some(incident)).await.unwrap();
}

async fn entry(
    store: &domovoi_store::Store,
    incident: domovoi_core::IncidentId,
    kind: &str,
    at: Timestamp,
) {
    let entry = IncidentTimelineEntry {
        incident,
        at,
        kind: kind.to_owned(),
        summary: format!("{kind} at {at}"),
        detail: serde_json::Value::Null,
    };
    store.append_event(&entry).await.unwrap();
}

#[tokio::test]
async fn purge_respects_retention_policy() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(100);

    let open = incident(host, "disk.free", None).unwrap();
    IncidentRepo::insert(store, &open).await.unwrap();
    for (age, check) in [(31, "old.signal"), (29, "new.signal")] {
        old_signal(store, host, open.id(), check, now - days(age)).await;
    }
    for age in [91, 89] {
        entry(store, open.id(), "status.changed", now - days(age)).await;
    }
    let fp = open.fingerprint();
    TriageCacheRepo::put(store, &fp, &[1; 32], &report(), now - days(91))
        .await
        .unwrap();
    TriageCacheRepo::put(store, &fp, &[2; 32], &report(), now - days(89))
        .await
        .unwrap();
    for (age, resolved) in [(31, true), (29, true), (200, false)] {
        let advisory = Advisory {
            id: AdvisoryId::new(),
            kind: AdvisoryKind::StaleImage,
            host: Some(host),
            title: format!("{age}"),
            detail: String::new(),
            created_at: now - days(age),
            resolved_at: resolved.then(|| now - days(age)),
        };
        AdvisoryRepo::upsert(store, &advisory).await.unwrap();
    }
    let audit = AuditEvent {
        at: now - days(400),
        actor: "system:test".to_owned(),
        kind: "test.old".to_owned(),
        subject: "x".to_owned(),
        detail: serde_json::Value::Null,
    };
    store.append(&audit).await.unwrap();

    let report = store.purge(now, &RetentionPolicy::default()).await.unwrap();
    assert_eq!(report.signals, 1);
    assert_eq!(report.timeline, 1);
    assert_eq!(report.conversations, 0);
    assert_eq!(report.notifications, 0);
    assert_eq!(report.triage, 1);
    assert_eq!(report.advisories, 1, "resolved more than 30 days ago");

    let left = store.list_for_incident(open.id(), 10).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].check().as_str(), "new.signal");
    assert_eq!(store.timeline(open.id()).await.unwrap().len(), 1);
    assert!(
        TriageCacheRepo::get(store, &fp, &[1; 32], T0)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        TriageCacheRepo::get(store, &fp, &[2; 32], T0)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store.list_open().await.unwrap().len(),
        1,
        "open advisories stay"
    );
    assert!(
        IncidentRepo::get(store, open.id()).await.unwrap().is_some(),
        "incidents stay"
    );
    let audit_left = AuditLog::list(store, &AuditFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(audit_left.items, vec![audit], "the audit log is kept");

    // Purging again finds nothing more.
    let again = store.purge(now, &RetentionPolicy::default()).await.unwrap();
    assert_eq!(again, PurgeReport::default());
}

#[tokio::test]
async fn purge_uses_the_configured_durations() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(100);
    let open = open_incident(host, "disk.free", None).unwrap();
    IncidentRepo::insert(store, &open).await.unwrap();

    old_signal(store, host, open.id(), "signal.20", now - days(20)).await;
    old_signal(store, host, open.id(), "signal.40", now - days(40)).await;
    entry(store, open.id(), "status.changed", now - days(20)).await;
    entry(store, open.id(), "status.changed", now - days(40)).await;
    let fp = open.fingerprint();
    TriageCacheRepo::put(store, &fp, &[20; 32], &report(), now - days(20))
        .await
        .unwrap();
    TriageCacheRepo::put(store, &fp, &[40; 32], &report(), now - days(40))
        .await
        .unwrap();
    for age in [20, 40] {
        let advisory = Advisory {
            id: AdvisoryId::new(),
            kind: AdvisoryKind::StaleImage,
            host: Some(host),
            title: format!("{age}"),
            detail: String::new(),
            created_at: now - days(age),
            resolved_at: Some(now - days(age)),
        };
        AdvisoryRepo::upsert(store, &advisory).await.unwrap();
    }

    let report = store.purge(now, &policy_days(30)).await.unwrap();
    assert_eq!(report.signals, 1, "the 40-day signal");
    assert_eq!(report.timeline, 1, "the 40-day entry");
    assert_eq!(report.triage, 1, "the 40-day report");
    assert_eq!(report.advisories, 1, "the 40-day resolved advisory");
    assert_eq!(
        store.list_for_incident(open.id(), 10).await.unwrap().len(),
        1
    );
    assert_eq!(store.timeline(open.id()).await.unwrap().len(), 1);
    assert!(
        TriageCacheRepo::get(store, &fp, &[20; 32], T0)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(store.list_open().await.unwrap().len(), 0);

    let report = store.purge(now, &policy_days(14)).await.unwrap();
    assert_eq!(report.signals, 1, "the 20-day signal");
    assert_eq!(report.timeline, 1, "the 20-day entry");
    assert_eq!(report.triage, 1, "the 20-day report");
    assert_eq!(report.advisories, 1, "the 20-day resolved advisory");
    assert!(
        store
            .list_for_incident(open.id(), 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.timeline(open.id()).await.unwrap().is_empty());
    assert!(
        TriageCacheRepo::get(store, &fp, &[20; 32], T0)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn conversations_are_kept_longer_than_the_timeline() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(300);
    let open = open_incident(host, "disk.free", None).unwrap();
    IncidentRepo::insert(store, &open).await.unwrap();

    entry(store, open.id(), FOLLOW_UP_KIND, now - days(120)).await;
    entry(store, open.id(), CLEAR_KIND, now - days(120)).await;
    entry(store, open.id(), "status.changed", now - days(120)).await;
    entry(store, open.id(), FOLLOW_UP_KIND, now - days(200)).await;
    entry(store, open.id(), CLEAR_KIND, now - days(200)).await;

    let policy = RetentionPolicy {
        timeline: days(90),
        conversations: days(180),
        ..policy_days(90)
    };
    let report = store.purge(now, &policy).await.unwrap();
    assert_eq!(report.timeline, 1, "the 120-day status change");
    assert_eq!(report.conversations, 2, "the 200-day question and clear");

    let left = store.timeline(open.id()).await.unwrap();
    let kinds: Vec<&str> = left.iter().map(|entry| entry.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![FOLLOW_UP_KIND, CLEAR_KIND],
        "120-day conversations survive a 180-day policy"
    );
}

#[tokio::test]
async fn notification_state_is_purged_only_for_resolved_incidents_not_held() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(100);

    let old_resolved = resolved_incident(host, "old.resolved", None, now - days(60)).unwrap();
    let held = resolved_incident(host, "held.resolved", None, now - days(60)).unwrap();
    let still_open = open_incident(host, "still.open", None).unwrap();
    let fresh_resolved = resolved_incident(host, "fresh.resolved", None, now - days(1)).unwrap();
    for incident in [&old_resolved, &held, &still_open, &fresh_resolved] {
        IncidentRepo::insert(store, incident).await.unwrap();
    }

    let stale = now - days(40);
    NotifyStateRepo::put(store, old_resolved.id(), &NotifyState::default(), stale)
        .await
        .unwrap();
    let held_state = NotifyState {
        held: Some(HeldItem {
            line: "disk full".to_owned(),
            severity: Severity::Warning,
            held_at: stale,
            resolved: false,
        }),
        ..NotifyState::default()
    };
    NotifyStateRepo::put(store, held.id(), &held_state, stale)
        .await
        .unwrap();
    NotifyStateRepo::put(store, still_open.id(), &NotifyState::default(), stale)
        .await
        .unwrap();
    NotifyStateRepo::put(
        store,
        fresh_resolved.id(),
        &NotifyState::default(),
        now - days(1),
    )
    .await
    .unwrap();

    let report = store.purge(now, &RetentionPolicy::default()).await.unwrap();
    assert_eq!(report.notifications, 1);
    assert!(
        NotifyStateRepo::get(store, old_resolved.id())
            .await
            .unwrap()
            .is_none(),
        "the state of a resolved incident 40 days old is removed"
    );
    assert!(
        NotifyStateRepo::get(store, held.id())
            .await
            .unwrap()
            .is_some(),
        "a held alert stays"
    );
    assert!(
        NotifyStateRepo::get(store, still_open.id())
            .await
            .unwrap()
            .is_some(),
        "the state of an open incident stays"
    );
    assert!(
        NotifyStateRepo::get(store, fresh_resolved.id())
            .await
            .unwrap()
            .is_some(),
        "a fresh resolved incident keeps its state"
    );
}

#[tokio::test]
async fn purge_is_all_or_nothing() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(100);
    let open = open_incident(host, "disk.free", None).unwrap();
    IncidentRepo::insert(store, &open).await.unwrap();
    old_signal(store, host, open.id(), "old.signal", now - days(40)).await;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, csrf_hash, created_at, expires_at) \
         VALUES (?, 'user:x', ?, ?, ?)",
    )
    .bind(vec![1_u8; 32])
    .bind(vec![2_u8; 32])
    .bind((now - days(2)).as_millisecond())
    .bind((now - days(1)).as_millisecond())
    .execute(store.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER boom BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'boom'); END",
    )
    .execute(store.pool_for_tests())
    .await
    .unwrap();

    assert!(store.purge(now, &RetentionPolicy::default()).await.is_err());
    assert_eq!(
        store.list_for_incident(open.id(), 10).await.unwrap().len(),
        1,
        "a failed purge deletes nothing"
    );

    sqlx::query("DROP TRIGGER boom")
        .execute(store.pool_for_tests())
        .await
        .unwrap();
    let report = store.purge(now, &RetentionPolicy::default()).await.unwrap();
    assert_eq!((report.signals, report.sessions), (1, 1));
    assert!(
        store
            .list_for_incident(open.id(), 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn purge_never_touches_the_audit_log_or_incidents() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let now: Timestamp = T0 + days(100);
    let open = open_incident(host, "disk.free", None).unwrap();
    IncidentRepo::insert(store, &open).await.unwrap();
    let audit = AuditEvent {
        at: now - days(1),
        actor: "system:test".to_owned(),
        kind: "test.kept".to_owned(),
        subject: "x".to_owned(),
        detail: serde_json::Value::Null,
    };
    store.append(&audit).await.unwrap();

    let report = store.purge(now, &policy_days(1)).await.unwrap();
    assert_eq!(report, PurgeReport::default());
    assert!(IncidentRepo::get(store, open.id()).await.unwrap().is_some());
    let audit_left = AuditLog::list(store, &AuditFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(audit_left.items, vec![audit]);
}

#[tokio::test]
async fn purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction() {
    let fresh = Fresh::new().await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, sql FROM sqlite_master WHERE sql LIKE 'CREATE VIRTUAL TABLE%'",
    )
    .fetch_all(fresh.store.pool_for_tests())
    .await
    .unwrap();
    assert!(
        rows.iter().any(|(name, _)| name == "incidents_fts"),
        "the fixtures create a full-text table"
    );
    for (name, sql) in rows {
        let Some(table) = content_table(&sql) else {
            panic!("{name} has no content table");
        };
        assert!(
            !PURGED_TABLES.contains(&table)
                || FTS_CLEANUP.iter().any(|(purged, _)| *purged == table),
            "{table} is purged but its full-text index is not cleaned up"
        );
    }
}

/// The `content = '<table>'` option of a virtual table's stored SQL.
fn content_table(sql: &str) -> Option<&str> {
    let rest = sql.split("content = '").nth(1)?;
    rest.split('\'').next()
}
