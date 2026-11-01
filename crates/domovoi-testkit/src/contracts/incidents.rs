//! Contracts for [`SignalRepo`] and [`IncidentRepo`].

use domovoi_core::ports::{
    IncidentFilter, IncidentRepo, IncidentTimelineEntry, Page, SignalRepo, StoreError,
};
use domovoi_core::{
    HostId, Incident, IncidentEvent, IncidentId, IncidentPolicy, Severity, Signal, SourceKind,
    apply,
};
use serde_json::json;

use super::ContractResult;
use crate::build::{BuildError, T0, at, incident, open_incident, resolved_incident, signal};

fn signal_at(host: HostId, check: &str, secs: i64) -> Result<Signal, BuildError> {
    let base = signal(host, check, None)?;
    Ok(Signal::firing(
        SourceKind::Agent,
        host,
        base.check().clone(),
        None,
        Severity::Warning,
        base.summary(),
        at(secs),
    ))
}

fn pending_at(host: HostId, check: &str, secs: i64) -> Result<Incident, BuildError> {
    Ok(Incident::new_pending(&signal(host, check, None)?, at(secs)))
}

fn resolve(incident: &mut Incident, secs: i64) -> Result<(), BuildError> {
    let event = IncidentEvent::Resolved {
        observed_at: at(secs),
    };
    apply(incident, event, &IncidentPolicy::default(), at(secs))?;
    Ok(())
}

/// Exercises linking, ordering and purging of signals on an empty `repo`.
pub async fn signal_repo_contract(repo: &dyn SignalRepo) -> ContractResult {
    let host = HostId::new();
    let (a, b) = (IncidentId::new(), IncidentId::new());
    let mut a_signals = Vec::new();
    for secs in [0, 20, 10] {
        let s = signal_at(host, "disk.smart", secs)?;
        repo.insert(&s, Some(a)).await?;
        a_signals.push(s);
    }
    repo.insert(&signal_at(host, "disk.smart", 5)?, Some(b))
        .await?;
    repo.insert(&signal_at(host, "load.high", 0)?, None).await?;
    assert!(
        matches!(
            repo.insert(&a_signals[0], Some(a)).await,
            Err(StoreError::Conflict(_))
        ),
        "signal ids are unique"
    );

    let times: Vec<_> = repo
        .list_for_incident(a, 10)
        .await?
        .iter()
        .map(Signal::observed_at)
        .collect();
    assert_eq!(times, [at(20), at(10), at(0)], "newest observed first");
    assert_eq!(repo.list_for_incident(a, 2).await?.len(), 2);
    assert!(
        repo.list_for_incident(IncidentId::new(), 10)
            .await?
            .is_empty()
    );

    assert_eq!(repo.purge_older_than(at(10)).await?, 3);
    assert_eq!(repo.list_for_incident(a, 10).await?.len(), 2);
    assert!(repo.list_for_incident(b, 10).await?.is_empty());
    assert_eq!(repo.purge_older_than(at(10)).await?, 0);
    Ok(())
}

/// Exercises the fingerprint rules, listing, timeline and search on an empty `repo`.
pub async fn incident_repo_contract(repo: &dyn IncidentRepo) -> ContractResult {
    fingerprints(repo).await?;
    listing(repo).await?;
    timeline(repo).await?;
    discard(repo).await?;
    search(repo).await
}

async fn discard(repo: &dyn IncidentRepo) -> ContractResult {
    let host = HostId::new();
    let pending = crate::build::incident(host, "net.flap", Some("eth0"))?;
    repo.insert(&pending).await?;
    let entry = IncidentTimelineEntry {
        incident: pending.id(),
        at: at(1),
        kind: "status.changed".to_owned(),
        summary: "pending".to_owned(),
        detail: serde_json::Value::Null,
    };
    repo.append_event(&entry).await?;
    repo.discard_pending(pending.id()).await?;
    assert_eq!(repo.get(pending.id()).await?, None, "discarded");
    assert!(
        repo.timeline(pending.id()).await?.is_empty(),
        "timeline goes too"
    );
    assert_eq!(
        repo.discard_pending(pending.id()).await,
        Err(StoreError::NotFound)
    );
    // The fingerprint is free again.
    let again = crate::build::incident(host, "net.flap", Some("eth0"))?;
    repo.insert(&again).await?;

    let open = open_incident(host, "net.down", None)?;
    repo.insert(&open).await?;
    assert!(
        matches!(
            repo.discard_pending(open.id()).await,
            Err(StoreError::Conflict(_))
        ),
        "confirmed incidents are kept"
    );
    assert_eq!(repo.get(open.id()).await?, Some(open));
    Ok(())
}

async fn fingerprints(repo: &dyn IncidentRepo) -> ContractResult {
    let host = HostId::new();
    let mut open = open_incident(host, "disk.smart", Some("/dev/sda"))?;
    let fp = open.fingerprint();
    assert_eq!(repo.find_active_by_fingerprint(&fp).await?, None);
    assert_eq!(repo.update(&open).await, Err(StoreError::NotFound));

    repo.insert(&open).await?;
    assert_eq!(repo.get(open.id()).await?, Some(open.clone()));
    assert!(matches!(
        repo.insert(&open).await,
        Err(StoreError::Conflict(_))
    ));
    let twin = open_incident(host, "disk.smart", Some("/dev/sda"))?;
    assert!(
        matches!(repo.insert(&twin).await, Err(StoreError::Conflict(_))),
        "one active incident per fingerprint"
    );
    assert_eq!(
        repo.find_active_by_fingerprint(&fp).await?,
        Some(open.clone())
    );

    resolve(&mut open, 100)?;
    repo.update(&open).await?;
    assert_eq!(repo.find_active_by_fingerprint(&fp).await?, None);
    assert_eq!(
        repo.find_recent_resolved_by_fingerprint(&fp, at(100))
            .await?,
        Some(open.clone())
    );
    assert_eq!(
        repo.find_recent_resolved_by_fingerprint(&fp, at(101))
            .await?,
        None,
        "resolved before the window"
    );

    repo.insert(&twin).await?;
    let reopened = reopen(open)?;
    assert!(
        matches!(repo.update(&reopened).await, Err(StoreError::Conflict(_))),
        "an update may not create a second active incident"
    );
    Ok(())
}

fn reopen(mut incident: Incident) -> Result<Incident, BuildError> {
    let firing = IncidentEvent::Firing {
        severity: Severity::Warning,
        class: domovoi_core::SignalClass::Immediate,
        observed_at: at(110),
        value: None,
    };
    apply(&mut incident, firing, &IncidentPolicy::default(), at(110))?;
    Ok(incident)
}

async fn listing(repo: &dyn IncidentRepo) -> ContractResult {
    let host = HostId::new();
    let checks = [
        "list.one",
        "list.two",
        "list.three",
        "list.four",
        "list.five",
    ];
    let mut ids = Vec::new();
    for (secs, check) in (1..).zip(checks) {
        let incident = pending_at(host, check, secs)?;
        ids.push(incident.id());
        repo.insert(&incident).await?;
    }
    ids.reverse();
    let only_host = IncidentFilter {
        host: Some(host),
        ..IncidentFilter::default()
    };

    let mut seen = Vec::new();
    let mut page = Page::first(2);
    loop {
        let result = repo.list(&only_host, &page).await?;
        assert!(result.items.len() <= 2, "page limit respected");
        seen.extend(result.items.iter().map(Incident::id));
        if seen.len() == 2 {
            // A newer incident inserted between pages must not shift later pages.
            repo.insert(&pending_at(host, "list.late", 100)?).await?;
        }
        match result.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen, ids, "newest opened first, no gaps or repeats");

    let first = repo.list(&only_host, &Page::first(10)).await?;
    assert_eq!(first.items.len(), 6);
    assert_eq!(first.next_cursor, None, "no cursor on the last page");

    // Opened at T0, so it falls outside `opened_since` below.
    repo.insert(&resolved_incident(host, "list.done", None, at(60))?)
        .await?;
    let filters = [
        (
            IncidentFilter {
                active: Some(false),
                ..only_host.clone()
            },
            1,
        ),
        (
            IncidentFilter {
                active: Some(true),
                ..only_host.clone()
            },
            6,
        ),
        (
            IncidentFilter {
                opened_since: Some(at(4)),
                ..only_host.clone()
            },
            3,
        ),
        (
            IncidentFilter {
                host: Some(HostId::new()),
                ..IncidentFilter::default()
            },
            0,
        ),
        (
            IncidentFilter {
                status: Some("resolved".to_owned()),
                ..only_host.clone()
            },
            1,
        ),
        (
            IncidentFilter {
                status: Some("acked".to_owned()),
                ..only_host.clone()
            },
            0,
        ),
        (
            IncidentFilter {
                severity: Some(crate::build::signal(host, "x.y", None)?.severity()),
                status: Some("resolved".to_owned()),
                ..only_host.clone()
            },
            1,
        ),
        (
            IncidentFilter {
                severity: Some(domovoi_core::Severity::Critical),
                ..only_host.clone()
            },
            0,
        ),
    ];
    for (filter, expected) in filters {
        let got = repo.list(&filter, &Page::first(100)).await?.items.len();
        assert_eq!(got, expected, "{filter:?}");
    }
    Ok(())
}

async fn timeline(repo: &dyn IncidentRepo) -> ContractResult {
    let incident = incident(HostId::new(), "timeline.check", None)?;
    let entry = |kind: &str, secs| IncidentTimelineEntry {
        incident: incident.id(),
        at: at(secs),
        kind: kind.to_owned(),
        summary: format!("{kind} happened"),
        detail: json!({ "n": secs }),
    };
    assert_eq!(
        repo.append_event(&entry("status.changed", 1)).await,
        Err(StoreError::NotFound)
    );
    repo.insert(&incident).await?;
    repo.append_event(&entry("signal.folded", 5)).await?;
    repo.append_event(&entry("status.changed", 3)).await?;
    let kinds: Vec<String> = repo
        .timeline(incident.id())
        .await?
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["signal.folded", "status.changed"], "append order");
    assert!(repo.timeline(IncidentId::new()).await?.is_empty());
    Ok(())
}

async fn search(repo: &dyn IncidentRepo) -> ContractResult {
    let host = HostId::new();
    let older = Incident::new_pending(&signal(host, "zfs.pool_degraded", None)?, T0);
    let newer = Incident::new_pending(&signal(host, "zfs.scrub_errors", None)?, at(10));
    repo.insert(&older).await?;
    repo.insert(&newer).await?;
    // Builder titles read "<check> firing", for example "zfs.scrub_errors firing".

    let ids = |hits: Vec<Incident>| hits.into_iter().map(|i| i.id()).collect::<Vec<_>>();
    assert_eq!(ids(repo.search("ZFS", 10).await?), [newer.id(), older.id()]);
    assert_eq!(ids(repo.search("zfs FIRING", 1).await?), [newer.id()]);
    assert_eq!(
        ids(repo.search("pool_degraded zfs", 10).await?),
        [older.id()]
    );
    for query in ["zf", "zfs missing", "", "  "] {
        assert!(
            repo.search(query, 10).await?.is_empty(),
            "`{query}` matched"
        );
    }
    Ok(())
}
