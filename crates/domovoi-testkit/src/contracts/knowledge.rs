//! Contracts for notes, baselines, ack rules, maintenance windows, advisories and facts.

use domovoi_core::ports::{AckRepo, AdvisoryRepo, BaselineRepo, FactRepo, NoteRepo, StoreError};
use domovoi_core::{
    AckRule, AckRuleId, Advisory, AdvisoryId, AdvisoryKind, BaselineEntry, BaselineKind, CheckKey,
    Criticality, DigestFact, FactValue, HostId, MaintenanceWindow, Note, NoteId,
};

use super::ContractResult;
use crate::build::{BuildError, T0, at};

fn note(host: Option<HostId>, title: &str, body: &str, secs: i64) -> Note {
    Note {
        id: NoteId::new(),
        host,
        title: title.to_owned(),
        body_markdown: body.to_owned(),
        tags: vec!["runbook".to_owned()],
        updated_at: at(secs),
    }
}

/// Exercises note storage, scoping and search on an empty `repo`.
pub async fn note_repo_contract(repo: &dyn NoteRepo) -> ContractResult {
    let (nas, other) = (HostId::new(), HostId::new());
    let global = note(
        None,
        "Power",
        "The UPS shuts the rack down after 10 minutes.",
        1,
    );
    let mut pool = note(
        Some(nas),
        "ZFS pool",
        "Scrub runs Sunday.\nZFS replace: zpool replace tank.",
        2,
    );
    let foreign = note(Some(other), "ZFS on other", "Other host zfs notes.", 3);
    for n in [&global, &pool, &foreign] {
        repo.upsert(n).await?;
    }
    assert_eq!(repo.get(pool.id).await?, Some(pool.clone()));
    assert_eq!(repo.get(NoteId::new()).await?, None);

    pool.body_markdown.push_str("\nzfs zfs zfs");
    pool.updated_at = at(10);
    repo.upsert(&pool).await?;
    assert_eq!(
        repo.get(pool.id).await?,
        Some(pool.clone()),
        "upsert replaces"
    );

    assert_eq!(repo.list(Some(nas)).await?, vec![pool.clone()]);
    assert_eq!(
        repo.list(None).await?,
        vec![global.clone()],
        "None lists global notes"
    );

    let hits = repo.search("zfs", Some(nas), 10).await?;
    let ids: Vec<NoteId> = hits.iter().map(|h| h.note.id).collect();
    assert_eq!(
        ids,
        [pool.id],
        "host scope searches that host and global notes"
    );
    assert!(
        hits.iter().all(|h| !h.snippet.is_empty()),
        "snippets are non-empty"
    );

    let everywhere = repo.search("ZFS", None, 10).await?;
    assert_eq!(everywhere.len(), 2, "None searches every note");
    assert!(
        everywhere.windows(2).all(|w| w[0].score >= w[1].score),
        "best score first"
    );
    assert_eq!(everywhere[0].note.id, pool.id, "more matches score higher");
    assert_eq!(
        repo.search("zfs", None, 1).await?.len(),
        1,
        "limit respected"
    );
    assert_eq!(
        repo.search("ups rack", Some(nas), 10).await?.len(),
        1,
        "global note found"
    );
    assert_eq!(
        repo.search("runbook", Some(other), 10).await?.len(),
        2,
        "tags are searched"
    );
    for query in ["zf", "zfs ups", ""] {
        assert!(
            repo.search(query, None, 10).await?.is_empty(),
            "`{query}` matched"
        );
    }

    repo.delete(global.id).await?;
    assert_eq!(repo.get(global.id).await?, None);
    assert_eq!(repo.delete(global.id).await, Err(StoreError::NotFound));
    Ok(())
}

fn entry(host: HostId, kind: BaselineKind, key: &str) -> BaselineEntry {
    BaselineEntry {
        host,
        kind,
        key: key.to_owned(),
        criticality: Criticality::Warning,
        learned_at: T0,
        last_seen_at: T0,
        managed: true,
    }
}

/// Exercises baseline upsert, listing and deletion on an empty `repo`.
pub async fn baseline_repo_contract(repo: &dyn BaselineRepo) -> ContractResult {
    let host = HostId::new();
    let plex = entry(host, BaselineKind::Container, "plex");
    let ssh = entry(host, BaselineKind::Port, "tcp/22");
    let sshd = entry(host, BaselineKind::Unit, "ssh.service");
    for e in [&ssh, &plex, &sshd] {
        repo.upsert(e).await?;
    }
    repo.upsert(&entry(HostId::new(), BaselineKind::Container, "plex"))
        .await?;

    let mut seen_again = plex.clone();
    seen_again.last_seen_at = at(100);
    seen_again.criticality = Criticality::Critical;
    repo.upsert(&seen_again).await?;

    let listed = repo.list(host).await?;
    let keys: Vec<&str> = listed.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(
        keys,
        ["plex", "ssh.service", "tcp/22"],
        "one per key, sorted by key"
    );
    assert_eq!(
        listed[0], seen_again,
        "upsert replaces by host, kind and key"
    );

    repo.delete(host, BaselineKind::Port, "tcp/22").await?;
    assert_eq!(repo.list(host).await?.len(), 2);
    assert_eq!(
        repo.delete(host, BaselineKind::Port, "tcp/22").await,
        Err(StoreError::NotFound)
    );
    assert_eq!(
        repo.delete(host, BaselineKind::Unit, "plex").await,
        Err(StoreError::NotFound),
        "kind is part of the identity"
    );
    Ok(())
}

/// Exercises ack rules and maintenance windows on an empty `repo`.
pub async fn ack_repo_contract(repo: &dyn AckRepo) -> ContractResult {
    let mut rule = AckRule {
        id: AckRuleId::new(),
        host: None,
        check: CheckKey::parse("disk.smart").map_err(BuildError::from)?,
        target_pattern: None,
        predicates: Vec::new(),
        note: "known".to_owned(),
        expires_at: Some(at(10)),
    };
    let other = AckRule {
        id: AckRuleId::new(),
        ..rule.clone()
    };
    repo.put_rule(&other).await?;
    repo.put_rule(&rule).await?;
    rule.note = "known, replaced in spring".to_owned();
    repo.put_rule(&rule).await?;
    let mut expected = vec![rule.clone(), other.clone()];
    expected.sort_by_key(|r| r.id);
    assert_eq!(
        repo.list_rules().await?,
        expected,
        "upsert by id, sorted by id, expired kept"
    );
    repo.delete_rule(other.id).await?;
    assert_eq!(repo.list_rules().await?, vec![rule.clone()]);
    assert_eq!(repo.delete_rule(other.id).await, Err(StoreError::NotFound));

    let host = HostId::new();
    let window = |start, end| MaintenanceWindow {
        host,
        starts_at: at(start),
        ends_at: at(end),
        note: "disk swap".to_owned(),
    };
    let (late, early) = (window(100, 200), window(0, 50));
    repo.put_window(&late).await?;
    repo.put_window(&early).await?;
    repo.put_window(&early).await?;
    repo.put_window(&MaintenanceWindow {
        host: HostId::new(),
        ..early.clone()
    })
    .await?;
    assert_eq!(
        repo.windows_for(host).await?,
        vec![early.clone(), late.clone()],
        "sorted by start, identical windows stored once"
    );
    assert_eq!(repo.purge_windows_ended_before(at(49)).await?, 0);
    assert_eq!(
        repo.purge_windows_ended_before(at(50)).await?,
        2,
        "ends_at <= before"
    );
    assert_eq!(repo.windows_for(host).await?, vec![late]);
    Ok(())
}

/// Exercises advisory upsert, listing and resolution on an empty `repo`.
pub async fn advisory_repo_contract(repo: &dyn AdvisoryRepo) -> ContractResult {
    let advisory = |kind, secs| Advisory {
        id: AdvisoryId::new(),
        kind,
        host: None,
        title: format!("{kind:?}"),
        detail: "check it".to_owned(),
        created_at: at(secs),
        resolved_at: None,
    };
    let older = advisory(AdvisoryKind::StaleImage, 1);
    let mut newer = advisory(AdvisoryKind::AgentOutdated, 2);
    repo.upsert(&older).await?;
    repo.upsert(&newer).await?;
    newer.detail = "upgrade the agent".to_owned();
    repo.upsert(&newer).await?;
    assert_eq!(
        repo.list_open().await?,
        vec![newer.clone(), older.clone()],
        "newest first"
    );

    repo.resolve(older.id, at(5)).await?;
    repo.resolve(older.id, at(9)).await?;
    assert_eq!(repo.list_open().await?, vec![newer]);
    assert_eq!(
        repo.resolve(AdvisoryId::new(), at(5)).await,
        Err(StoreError::NotFound)
    );
    Ok(())
}

/// Exercises fact recording and range queries on an empty `repo`.
pub async fn fact_repo_contract(repo: &dyn FactRepo) -> ContractResult {
    let host = HostId::new();
    let fact = |host, secs, value: f64| DigestFact {
        host,
        key: "zfs.tank.used_pct".to_owned(),
        label: "tank used".to_owned(),
        value: FactValue::Number(value),
        unit: Some("%".to_owned()),
        observed_at: at(secs),
    };
    for (secs, value) in [(30, 71.0), (10, 70.0), (60, 72.0), (10, 70.5)] {
        repo.record(&fact(host, secs, value)).await?;
    }
    repo.record(&fact(HostId::new(), 20, 10.0)).await?;

    let day = repo.list(host, at(10), at(60)).await?;
    let times: Vec<_> = day.iter().map(|f| f.observed_at).collect();
    assert_eq!(
        times,
        [at(10), at(10), at(30)],
        "half-open range, oldest first, never replaced"
    );
    assert!(repo.list(host, at(61), at(100)).await?.is_empty());
    Ok(())
}
