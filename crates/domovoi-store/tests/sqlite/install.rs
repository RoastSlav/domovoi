//! Package artifacts, install jobs and host inventories in SQLite, and the same rules in the
//! in-memory twin.

use domovoi_core::ExecutionId;
use domovoi_core::package::{
    ArchiveDigest, KeyId, KeyState, PackageName, PackageStatus, PackageVersion, PrivilegeBoundary,
};
use domovoi_core::ports::{
    AggregateRef, AuditDraft, AuditFilter, AuditLog, CommitError, Expectation, InstallJobFilter,
    InstallRepo, InstallWrite, Observed, Page, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{
    Actor, ApprovalChannel, ApprovalProvenance, ArtifactGuard, ArtifactSource, HostId, HostPackage,
    HumanApproval, InstallJob, InstallJobId, InstallKind, InstallTarget, InstallTargetId,
    ObservedBy, PackageArtifact, PackageArtifactId, PlanHash, StoredAuthorization, TargetFailure,
    TargetGuard, TargetStatus, UserId,
};
use domovoi_store::Store;
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::at;
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::common::Fresh;

fn digest(byte: u8) -> ArchiveDigest {
    ArchiveDigest::of_bytes(&[byte; 8])
}

fn artifact(name: &str, version: &str, byte: u8) -> PackageArtifact {
    PackageArtifact {
        id: PackageArtifactId::new(),
        name: name.parse().unwrap(),
        version: version.parse().unwrap(),
        archive_sha256: digest(byte),
        key_id: KeyId::parse("0123456789abcdef").unwrap(),
        archive_size: 1024,
        signature_toml: "format = 1\n".to_owned(),
        signer_label: Some("Owner".to_owned()),
        source: ArtifactSource::OwnerImport,
        imported_by: Actor::User(UserId::new()),
        imported_at: at(10),
    }
}

fn plan_hash() -> PlanHash {
    "ab".repeat(32).parse().unwrap()
}

fn job(kind: InstallKind, created: Timestamp) -> InstallJob {
    let install = kind == InstallKind::Install;
    InstallJob {
        id: InstallJobId::new(),
        kind,
        name: "tools".parse().unwrap(),
        version: "1.0.0".parse().unwrap(),
        archive_sha256: install.then(|| digest(1)),
        key_id: install.then(|| KeyId::parse("0123456789abcdef").unwrap()),
        requested_by: UserId::new(),
        created_at: created,
        job_plan_hash: plan_hash(),
    }
}

fn target(job: &InstallJob, host: HostId, status: TargetStatus, finished: i64) -> InstallTarget {
    InstallTarget {
        id: InstallTargetId::new(),
        job: job.id,
        host,
        attempt: 1,
        execution: ExecutionId::new(),
        status,
        plan_hash: plan_hash(),
        authorization: StoredAuthorization::new(
            HumanApproval::new(
                UserId::new(),
                ApprovalChannel::Web {
                    typed_confirmation: true,
                },
                at(20),
            ),
            ApprovalProvenance::new(Some("ref".to_owned()), "web"),
        ),
        expires_at: at(900),
        artifact_transfer: None,
        signature_transfer: None,
        dispatched_session: None,
        dispatched_epoch: None,
        failure: None,
        outputs: None,
        outputs_confirmed: false,
        queued_at: at(20),
        started_at: None,
        finished_at: status.is_terminal().then(|| at(finished)),
        version: 1,
    }
}

fn create(job: &InstallJob, targets: &[InstallTarget]) -> Transaction {
    Transaction::new().write(StateWrite::Install(InstallWrite::CreateJob {
        job: job.clone(),
        targets: targets.to_vec(),
    }))
}

fn audit(kind: &str) -> AuditDraft {
    AuditDraft::new(
        Actor::System("packages".to_owned()),
        at(21),
        kind.to_owned(),
        "package:tools@1.0.0".to_owned(),
        json!({}),
    )
}

fn entry(host: HostId, name: &str, version: &str) -> HostPackage {
    HostPackage {
        host,
        name: name.parse::<PackageName>().unwrap(),
        version: version.parse::<PackageVersion>().unwrap(),
        key_id: KeyId::parse("0123456789abcdef").unwrap(),
        archive_sha256: digest(1),
        installed_at: at(30),
        status: PackageStatus::Active,
        key_state: KeyState::Trusted,
        skills: vec!["restart".to_owned()],
        privilege_boundary: PrivilegeBoundary::InEffect,
        observed_at: at(31),
        observed_by: ObservedBy::Job,
    }
}

fn inventory(host: HostId, entries: Vec<HostPackage>, replace: bool) -> Transaction {
    Transaction::new().write(StateWrite::Install(InstallWrite::Inventory {
        host,
        entries,
        replace,
    }))
}

async fn audit_count<S: AuditLog>(store: &S) -> usize {
    store
        .list(&AuditFilter::default(), &Page::first(100))
        .await
        .unwrap()
        .items
        .len()
}

async fn create_job_is_atomic_with_audit_on<S: UnitOfWork + InstallRepo + AuditLog>(s: &S) {
    let j = job(InstallKind::Install, at(20));
    let (a, b) = (HostId::new(), HostId::new());
    let targets = [
        target(&j, a, TargetStatus::Queued, 0),
        target(&j, b, TargetStatus::Queued, 0),
    ];
    s.commit(create(&j, &targets).audit(audit("package.install_requested")))
        .await
        .unwrap();
    assert_eq!(audit_count(s).await, 1);
    assert_eq!(s.job(j.id).await.unwrap(), Some(j.clone()));
    let stored = s.targets(j.id).await.unwrap();
    assert_eq!(stored.len(), 2);
    let mut expected = targets.to_vec();
    expected.sort_by_key(|t| t.host.to_string());
    assert_eq!(stored, expected, "the approval and every field round-trip");

    // A failing step later in the same transaction removes the job, its targets and the audit.
    let other = job(InstallKind::Install, at(21));
    let tx = create(
        &other,
        &[target(&other, HostId::new(), TargetStatus::Queued, 0)],
    )
    .audit(audit("package.install_requested"))
    .write(StateWrite::Install(InstallWrite::ImportArtifact(artifact(
        "tools", "1.0.0", 1,
    ))))
    .write(StateWrite::Install(InstallWrite::ImportArtifact(artifact(
        "tools", "1.0.0", 1,
    ))));
    assert!(matches!(
        s.commit(tx).await,
        Err(CommitError::Constraint(_))
    ));
    assert!(s.job(other.id).await.unwrap().is_none());
    assert!(s.targets(other.id).await.unwrap().is_empty());
    assert_eq!(audit_count(s).await, 1);
}

async fn second_active_target_fails_the_whole_job_on<S: UnitOfWork + InstallRepo>(s: &S) {
    let busy = HostId::new();
    let first = job(InstallKind::Install, at(20));
    s.commit(create(
        &first,
        &[target(&first, busy, TargetStatus::Queued, 0)],
    ))
    .await
    .unwrap();

    let second = job(InstallKind::Install, at(21));
    let free = HostId::new();
    let tx = create(
        &second,
        &[
            target(&second, free, TargetStatus::Queued, 0),
            target(&second, busy, TargetStatus::Queued, 0),
        ],
    );
    match s.commit(tx).await {
        Err(CommitError::Constraint(message)) => {
            assert!(message.contains(&busy.to_string()), "{message}");
        }
        other => panic!("expected a constraint, got {other:?}"),
    }
    assert!(s.job(second.id).await.unwrap().is_none());
    assert!(s.targets(second.id).await.unwrap().is_empty());
    assert_eq!(s.non_terminal_targets().await.unwrap().len(), 1);

    // The host is free again once its target is terminal, and the same host twice in one job
    // is refused too.
    let mut done = s.targets(first.id).await.unwrap().remove(0);
    done.status = TargetStatus::NotRun;
    done.failure = Some(TargetFailure::new("not_run", "nothing started"));
    done.finished_at = Some(at(25));
    s.commit(
        Transaction::new().write(StateWrite::Install(InstallWrite::Target {
            target: done,
            expected: TargetGuard {
                status: TargetStatus::Queued,
                version: 1,
            },
        })),
    )
    .await
    .unwrap();
    let third = job(InstallKind::Install, at(26));
    let twice = [
        target(&third, busy, TargetStatus::Queued, 0),
        target(&third, busy, TargetStatus::Queued, 0),
    ];
    assert!(matches!(
        s.commit(create(&third, &twice)).await,
        Err(CommitError::Constraint(_))
    ));
    s.commit(create(&third, &twice[..1])).await.unwrap();
}

async fn stale_target_write_conflicts_on<S: UnitOfWork + InstallRepo>(s: &S) {
    let j = job(InstallKind::Install, at(20));
    let t = target(&j, HostId::new(), TargetStatus::Queued, 0);
    s.commit(create(&j, std::slice::from_ref(&t)))
        .await
        .unwrap();

    let write = |next: TargetStatus, guard: TargetGuard| {
        let mut changed = t.clone();
        changed.status = next;
        Transaction::new().write(StateWrite::Install(InstallWrite::Target {
            target: changed,
            expected: guard,
        }))
    };
    s.commit(write(
        TargetStatus::Transferring,
        TargetGuard {
            status: TargetStatus::Queued,
            version: 1,
        },
    ))
    .await
    .unwrap();
    let stored = s.target(t.id).await.unwrap().unwrap();
    assert_eq!(stored.status, TargetStatus::Transferring);
    assert_eq!(stored.version, 2, "the store bumps the version");

    let stale_version = write(
        TargetStatus::Dispatched,
        TargetGuard {
            status: TargetStatus::Transferring,
            version: 1,
        },
    );
    match s.commit(stale_version).await {
        Err(CommitError::Conflict {
            aggregate,
            expected,
            found,
        }) => {
            assert_eq!(aggregate, AggregateRef::install_job(j.id));
            assert_eq!(expected, Expectation::Version(1));
            assert_eq!(found, Observed::Version(2));
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    let stale_status = write(
        TargetStatus::Dispatched,
        TargetGuard {
            status: TargetStatus::Queued,
            version: 2,
        },
    );
    assert!(matches!(
        s.commit(stale_status).await,
        Err(CommitError::Conflict {
            expected: Expectation::Status(_),
            found: Observed::Status(_),
            ..
        })
    ));
    assert_eq!(
        s.target(t.id).await.unwrap().unwrap().status,
        TargetStatus::Transferring
    );
}

async fn artifact_conflicts_and_delete_guards_on<S: UnitOfWork + InstallRepo>(s: &S) {
    let first = artifact("tools", "1.0.0", 1);
    let import = |a: &PackageArtifact| {
        Transaction::new().write(StateWrite::Install(InstallWrite::ImportArtifact(a.clone())))
    };
    s.commit(import(&first)).await.unwrap();
    let other_digest = artifact("tools", "1.0.0", 2);
    assert!(matches!(
        s.commit(import(&other_digest)).await,
        Err(CommitError::Constraint(_))
    ));
    s.commit(import(&artifact("tools", "1.1.0", 1)))
        .await
        .unwrap();
    assert_eq!(s.artifacts().await.unwrap().len(), 2);
    assert_eq!(
        s.artifact(&first.name, &first.version).await.unwrap(),
        Some(first.clone())
    );
    assert_eq!(s.artifact_by_digest(&digest(1)).await.unwrap().len(), 2);

    // A running job pins its artifact.
    let j = job(InstallKind::Install, at(20));
    let t = target(&j, HostId::new(), TargetStatus::Queued, 0);
    s.commit(create(&j, std::slice::from_ref(&t)))
        .await
        .unwrap();
    let delete = Transaction::new().write(StateWrite::Install(InstallWrite::DeleteArtifact {
        id: first.id,
        expected: ArtifactGuard {
            archive_sha256: first.archive_sha256,
        },
    }));
    assert!(matches!(
        s.commit(delete).await,
        Err(CommitError::Constraint(_))
    ));
    let mut done = t.clone();
    done.status = TargetStatus::Failed;
    done.failure = Some(TargetFailure::new("agent_refused", "no"));
    done.finished_at = Some(at(30));
    s.commit(
        Transaction::new().write(StateWrite::Install(InstallWrite::Target {
            target: done,
            expected: TargetGuard {
                status: TargetStatus::Queued,
                version: 1,
            },
        })),
    )
    .await
    .unwrap();
    let wrong_guard = Transaction::new().write(StateWrite::Install(InstallWrite::DeleteArtifact {
        id: first.id,
        expected: ArtifactGuard {
            archive_sha256: digest(9),
        },
    }));
    assert!(s.commit(wrong_guard).await.is_err());
    let delete = Transaction::new().write(StateWrite::Install(InstallWrite::DeleteArtifact {
        id: first.id,
        expected: ArtifactGuard {
            archive_sha256: first.archive_sha256,
        },
    }));
    s.commit(delete).await.unwrap();
    assert!(
        s.artifact(&first.name, &first.version)
            .await
            .unwrap()
            .is_none()
    );
}

async fn inventory_replace_is_atomic_on<S: UnitOfWork + InstallRepo>(s: &S) {
    let host = HostId::new();
    let other = HostId::new();
    s.commit(inventory(
        host,
        vec![entry(host, "alpha", "1.0.0"), entry(host, "beta", "1.0.0")],
        true,
    ))
    .await
    .unwrap();
    s.commit(inventory(other, vec![entry(other, "alpha", "1.0.0")], true))
        .await
        .unwrap();

    // Replace keeps the listed rows (updated), drops the rest and leaves other hosts alone.
    s.commit(inventory(
        host,
        vec![entry(host, "beta", "2.0.0"), entry(host, "gamma", "1.0.0")],
        true,
    ))
    .await
    .unwrap();
    let names: Vec<(String, String)> = s
        .inventory(host)
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.name.to_string(), p.version.to_string()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("beta".to_owned(), "2.0.0".to_owned()),
            ("gamma".to_owned(), "1.0.0".to_owned())
        ]
    );
    assert_eq!(s.inventory(other).await.unwrap().len(), 1);
    assert_eq!(
        s.hosts_with(&"alpha".parse().unwrap(), &"1.0.0".parse().unwrap())
            .await
            .unwrap()
            .len(),
        1
    );

    // A failure later in the transaction undoes the replace.
    let tx = inventory(host, vec![entry(host, "delta", "1.0.0")], true).write(StateWrite::Install(
        InstallWrite::Inventory {
            host,
            entries: vec![entry(other, "bad", "1.0.0")],
            replace: false,
        },
    ));
    assert!(s.commit(tx).await.is_err());
    assert_eq!(s.inventory(host).await.unwrap().len(), 2);

    // A merge keeps rows that are not listed, and a removal deletes one row.
    s.commit(inventory(host, vec![entry(host, "delta", "1.0.0")], false))
        .await
        .unwrap();
    assert_eq!(s.inventory(host).await.unwrap().len(), 3);
    s.commit(
        Transaction::new().write(StateWrite::Install(InstallWrite::RemoveInventory {
            host,
            name: "delta".parse().unwrap(),
        })),
    )
    .await
    .unwrap();
    assert_eq!(s.inventory(host).await.unwrap().len(), 2);
}

macro_rules! both {
    ($($name:ident => $generic:ident),* $(,)?) => {$(
        #[tokio::test]
        async fn $name() {
            let fresh = Fresh::new().await;
            $generic(&fresh.store).await;
        }
    )*};
}

both! {
    create_job_is_atomic_with_audit => create_job_is_atomic_with_audit_on,
    second_active_target_for_a_host_fails_the_whole_job => second_active_target_fails_the_whole_job_on,
    target_write_with_stale_version_conflicts => stale_target_write_conflicts_on,
    artifact_conflict_on_name_version_other_digest => artifact_conflicts_and_delete_guards_on,
    inventory_replace_is_atomic => inventory_replace_is_atomic_on,
}

#[tokio::test]
async fn in_memory_store_follows_the_same_install_rules() {
    create_job_is_atomic_with_audit_on(&InMemoryStore::new()).await;
    second_active_target_fails_the_whole_job_on(&InMemoryStore::new()).await;
    stale_target_write_conflicts_on(&InMemoryStore::new()).await;
    artifact_conflicts_and_delete_guards_on(&InMemoryStore::new()).await;
    inventory_replace_is_atomic_on(&InMemoryStore::new()).await;
}

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * n)
}

async fn finish(store: &Store, t: &InstallTarget, status: TargetStatus, at_secs: i64) {
    let mut done = t.clone();
    done.status = status;
    done.finished_at = Some(at(at_secs));
    store
        .commit(
            Transaction::new().write(StateWrite::Install(InstallWrite::Target {
                target: done,
                expected: TargetGuard {
                    status: t.status,
                    version: t.version,
                },
            })),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn purge_keeps_jobs_with_active_targets() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let j = job(InstallKind::Install, at(1));
    let finished = target(&j, HostId::new(), TargetStatus::Queued, 0);
    let running = target(&j, HostId::new(), TargetStatus::Queued, 0);
    store
        .commit(create(&j, &[finished.clone(), running]))
        .await
        .unwrap();
    finish(store, &finished, TargetStatus::Succeeded, 2).await;

    let long_after = at(2 + 24 * 3600 * 400);
    assert_eq!(
        store
            .purge_install_jobs(long_after, days(180))
            .await
            .unwrap(),
        0
    );
    assert!(store.job(j.id).await.unwrap().is_some());
    assert_eq!(store.targets(j.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn purge_deletes_old_terminal_jobs_but_not_audit() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let old = job(InstallKind::Install, at(1));
    let old_target = target(&old, HostId::new(), TargetStatus::Queued, 0);
    let recent = job(InstallKind::Install, at(24 * 3600 * 300));
    let recent_target = target(&recent, HostId::new(), TargetStatus::Queued, 0);
    store
        .commit(create(&old, std::slice::from_ref(&old_target)).audit(audit("package.installed")))
        .await
        .unwrap();
    store
        .commit(create(&recent, std::slice::from_ref(&recent_target)))
        .await
        .unwrap();
    finish(store, &old_target, TargetStatus::Succeeded, 2).await;
    finish(
        store,
        &recent_target,
        TargetStatus::Succeeded,
        24 * 3600 * 300 + 1,
    )
    .await;
    store
        .commit(inventory(
            old_target.host,
            vec![entry(old_target.host, "tools", "1.0.0")],
            true,
        ))
        .await
        .unwrap();
    store
        .commit(
            Transaction::new().write(StateWrite::Install(InstallWrite::ImportArtifact(artifact(
                "tools", "1.0.0", 1,
            )))),
        )
        .await
        .unwrap();

    let now = at(24 * 3600 * 400);
    assert_eq!(store.purge_install_jobs(now, days(180)).await.unwrap(), 1);
    assert!(store.job(old.id).await.unwrap().is_none());
    assert!(
        store.targets(old.id).await.unwrap().is_empty(),
        "targets cascade"
    );
    assert!(store.job(recent.id).await.unwrap().is_some());
    assert_eq!(audit_count(store).await, 1);
    assert_eq!(store.artifacts().await.unwrap().len(), 1);
    assert_eq!(store.inventory(old_target.host).await.unwrap().len(), 1);
    let listed = store
        .jobs(&InstallJobFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(listed.items.len(), 1);
}

#[tokio::test]
async fn jobs_list_newest_first_with_filters() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let older = job(InstallKind::Install, at(10));
    let newer = job(InstallKind::Uninstall, at(20));
    store
        .commit(create(
            &older,
            &[target(&older, host, TargetStatus::Succeeded, 11)],
        ))
        .await
        .unwrap();
    store
        .commit(create(
            &newer,
            &[target(&newer, host, TargetStatus::Queued, 0)],
        ))
        .await
        .unwrap();
    let all = store
        .jobs(&InstallJobFilter::default(), &Page::first(1))
        .await
        .unwrap();
    assert_eq!(all.items[0].id, newer.id);
    assert!(all.next_cursor.is_some());
    let active = InstallJobFilter {
        active: Some(true),
        ..InstallJobFilter::default()
    };
    let items = store.jobs(&active, &Page::first(10)).await.unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, newer.id);
    let finished = InstallJobFilter {
        active: Some(false),
        host: Some(host),
        ..InstallJobFilter::default()
    };
    let items = store.jobs(&finished, &Page::first(10)).await.unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, older.id);
}
