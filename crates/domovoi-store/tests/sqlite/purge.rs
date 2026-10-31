//! Retention deletes old signals, timeline entries, cache entries and resolved advisories,
//! and never incidents or the audit log.

use domovoi_core::ports::{
    AdvisoryRepo, AuditEvent, AuditFilter, AuditLog, IncidentRepo, IncidentTimelineEntry, Page,
    SignalRepo, TriageCacheRepo,
};
use domovoi_core::{Advisory, AdvisoryId, AdvisoryKind, HostId, TriageReport, Verdict};
use domovoi_store::RetentionPolicy;
use domovoi_testkit::build::{T0, incident, signal};
use jiff::{SignedDuration, Timestamp};

use crate::common::Fresh;

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * n)
}

fn report() -> TriageReport {
    TriageReport {
        headline: "Disk filling".to_owned(),
        summary: "Logs grow.".to_owned(),
        cause: "Rotation is off.".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::Monitor,
        next_step: "Rotate logs.".to_owned(),
        proposals: Vec::new(),
        details: String::new(),
    }
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
        let s = signal(host, check, None).unwrap();
        let s = domovoi_core::Signal::firing(
            s.source().clone(),
            host,
            s.check().clone(),
            None,
            s.severity(),
            s.summary(),
            now - days(age),
        );
        SignalRepo::insert(store, &s, Some(open.id()))
            .await
            .unwrap();
    }
    for age in [91, 89] {
        let entry = IncidentTimelineEntry {
            incident: open.id(),
            at: now - days(age),
            kind: "status.changed".to_owned(),
            summary: format!("{age} days ago"),
            detail: serde_json::Value::Null,
        };
        store.append_event(&entry).await.unwrap();
    }
    let fp = open.fingerprint();
    store
        .put(&fp, &[1; 32], &report(), now - days(91))
        .await
        .unwrap();
    store
        .put(&fp, &[2; 32], &report(), now - days(89))
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
    assert_eq!(again, domovoi_store::PurgeReport::default());
}
