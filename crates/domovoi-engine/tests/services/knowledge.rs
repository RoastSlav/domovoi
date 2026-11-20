//! The knowledge service over the in-memory store: facts, their lifecycle and their audit.

use std::sync::Arc;

use domovoi_core::knowledge::{FactSource, KnowledgeStatus, Predicate, SubjectRef};
use domovoi_core::ports::{AuditFilter, AuditLog, FactFilter, JournalRead, KnowledgeRead, Page};
use domovoi_core::{Actor, HostId, UserId};
use domovoi_engine::knowledge::{
    FactDraft, KnowledgeDeps, KnowledgeError, KnowledgeService, RecordEffect, Recorded,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FakeCoverage, InMemoryStore};
use jiff::SignedDuration;
use serde_json::{Value, json};

pub const DAY: i64 = 86_400;

pub struct Rig {
    pub service: KnowledgeService,
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub coverage: Arc<FakeCoverage>,
}

pub fn rig() -> Rig {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let coverage = Arc::new(FakeCoverage::unavailable());
    let service = KnowledgeService::new(KnowledgeDeps {
        uow: store.clone(),
        read: store.clone(),
        coverage: coverage.clone(),
        clock: clock.clone(),
    });
    Rig {
        service,
        store,
        clock,
        coverage,
    }
}

pub fn system() -> Actor {
    Actor::System("knowledge".to_owned())
}

pub fn draft(
    subject: &SubjectRef,
    predicate: Predicate,
    value: Value,
    source: FactSource,
) -> FactDraft {
    FactDraft {
        subject: subject.clone(),
        predicate,
        value,
        source,
        observed_at: None,
        valid_until: None,
        evidence: None,
    }
}

pub fn host() -> SubjectRef {
    SubjectRef::Host(HostId::new())
}

pub async fn audit(r: &Rig, kind: &str) -> Vec<domovoi_core::ports::AuditEvent> {
    r.store
        .list(
            &AuditFilter {
                kind: Some(kind.to_owned()),
                ..AuditFilter::default()
            },
            &Page::first(100),
        )
        .await
        .unwrap()
        .items
}

pub async fn all_audit(r: &Rig) -> Vec<domovoi_core::ports::AuditEvent> {
    r.store
        .list(&AuditFilter::default(), &Page::first(500))
        .await
        .unwrap()
        .items
}

pub async fn journal_len(r: &Rig) -> usize {
    r.store.entries_after(0, 500).await.unwrap().len()
}

pub async fn owner_fact(r: &Rig, s: &SubjectRef, p: Predicate, v: Value) -> Recorded {
    r.service
        .assert_fact(draft(s, p, v, FactSource::User), UserId::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn inferred_fact_never_becomes_confirmed_without_user() {
    let r = rig();
    let s = host();
    let rec = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("nas"), FactSource::Inferred),
            &system(),
        )
        .await
        .unwrap();
    assert_eq!(rec.fact.value.status, KnowledgeStatus::Current);
    assert_eq!(rec.fact.value.source, FactSource::Inferred);
    assert!(!rec.fact.value.usable_for_safety(r.clock_now()));
    for actor in [
        system(),
        Actor::Agent(HostId::new()),
        Actor::Policy("p".into()),
    ] {
        let err = r
            .service
            .record(
                draft(&s, Predicate::Role, json!("x"), FactSource::User),
                &actor,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, KnowledgeError::ActorNotAllowed { .. }));
    }
    let accepted = r
        .service
        .accept(rec.fact.value.id, rec.fact.version, UserId::new())
        .await
        .unwrap();
    assert_eq!(accepted.fact.value.source, FactSource::User);
    assert_eq!(accepted.fact.value.supersedes, Some(rec.fact.value.id));
    let old = r.store.fact(rec.fact.value.id).await.unwrap().unwrap();
    assert_eq!(old.value.status, KnowledgeStatus::Superseded);
}

impl Rig {
    pub fn clock_now(&self) -> jiff::Timestamp {
        use domovoi_core::ports::Clock;
        self.clock.now()
    }
}

#[tokio::test]
async fn retracted_fact_not_retrievable() {
    let r = rig();
    let s = host();
    let rec = owner_fact(&r, &s, Predicate::Role, json!("media server")).await;
    let hits = r.service.search("media", None, 10).await.unwrap();
    assert_eq!(hits.len(), 1);
    r.service
        .retract(rec.fact.value.id, rec.fact.version, UserId::new())
        .await
        .unwrap();
    assert!(
        r.service
            .search("media", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let page = Page::first(50);
    let listed = r
        .service
        .facts(&FactFilter::default(), &page)
        .await
        .unwrap();
    assert!(listed.items.is_empty());
    assert!(
        r.store
            .live_facts(&s, Predicate::Role)
            .await
            .unwrap()
            .is_empty()
    );
    let retracted = r
        .service
        .facts(
            &FactFilter {
                statuses: Some(vec![KnowledgeStatus::Retracted]),
                ..FactFilter::default()
            },
            &page,
        )
        .await
        .unwrap();
    assert_eq!(retracted.items.len(), 1);
}

#[tokio::test]
async fn conflicting_sources_kept_as_disputed() {
    let r = rig();
    let s = host();
    r.service
        .record(
            draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
    let second = r
        .service
        .record(
            draft(
                &s,
                Predicate::Role,
                json!("backup"),
                FactSource::Integration,
            ),
            &system(),
        )
        .await
        .unwrap();
    assert_eq!(second.effect, RecordEffect::Disputed);
    let live = r.store.all_live_facts(100).await.unwrap();
    assert_eq!(live.len(), 2);
    for v in &live {
        assert_eq!(v.value.status, KnowledgeStatus::Disputed);
        assert!(!v.value.usable_for_safety(r.clock_now()));
    }
}

#[tokio::test]
async fn only_a_human_actor_can_write_a_user_sourced_fact() {
    let r = rig();
    let s = host();
    for actor in [
        system(),
        Actor::Agent(HostId::new()),
        Actor::Policy("p".into()),
    ] {
        let err = r
            .service
            .record(
                draft(&s, Predicate::Role, json!("x"), FactSource::User),
                &actor,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            KnowledgeError::ActorNotAllowed {
                source: FactSource::User
            }
        ));
    }
    assert!(r.store.all_live_facts(10).await.unwrap().is_empty());
    assert_eq!(journal_len(&r).await, 0);
    assert!(all_audit(&r).await.is_empty());
    owner_fact(&r, &s, Predicate::Role, json!("x")).await;
}

#[tokio::test]
async fn owner_accept_makes_an_inferred_fact_user_sourced_and_supersedes_it() {
    let r = rig();
    let s = host();
    let mut d = draft(
        &s,
        Predicate::OwnerNote,
        json!("restart me last"),
        FactSource::Inferred,
    );
    d.evidence = Some(format!("note:{}", uuid::Uuid::now_v7()).parse().unwrap());
    let rec = r.service.record(d, &system()).await.unwrap();
    let accepted = r
        .service
        .accept(rec.fact.value.id, rec.fact.version, UserId::new())
        .await
        .unwrap();
    let new = &accepted.fact.value;
    assert_eq!(
        (new.source, new.status),
        (FactSource::User, KnowledgeStatus::Current)
    );
    assert_eq!(new.supersedes, Some(rec.fact.value.id));
    assert_eq!(new.evidence, rec.fact.value.evidence);
    let events = audit(&r, "knowledge.fact.accepted").await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].detail["from"],
        json!(rec.fact.value.id.to_string())
    );
}

#[tokio::test]
async fn same_source_new_value_supersedes_the_old_fact() {
    let r = rig();
    let s = host();
    let value = |k: &str| json!({"hostname": "n", "os": "d", "kernel": k, "arch": "x", "cpu_count": 4, "mem_total_bytes": 8});
    let first = r
        .service
        .record(
            draft(
                &s,
                Predicate::HostPlatform,
                value("6.1"),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    let second = r
        .service
        .record(
            draft(
                &s,
                Predicate::HostPlatform,
                value("6.2"),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    let old = r.store.fact(first.fact.value.id).await.unwrap().unwrap();
    assert_eq!(old.value.status, KnowledgeStatus::Superseded);
    assert_eq!(second.fact.value.status, KnowledgeStatus::Current);
    assert_eq!(second.fact.value.supersedes, Some(first.fact.value.id));
}

#[tokio::test]
async fn owner_assertion_resolves_a_dispute() {
    let r = rig();
    let s = host();
    let a = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
    let b = r
        .service
        .record(
            draft(
                &s,
                Predicate::Role,
                json!("backup"),
                FactSource::Integration,
            ),
            &system(),
        )
        .await
        .unwrap();
    let won = owner_fact(&r, &s, Predicate::Role, json!("nas")).await;
    assert_eq!(won.fact.value.status, KnowledgeStatus::Current);
    for id in [a.fact.value.id, b.fact.value.id] {
        let old = r.store.fact(id).await.unwrap().unwrap();
        assert_eq!(old.value.status, KnowledgeStatus::Superseded);
    }
}

#[tokio::test]
async fn agreement_of_sources_resolves_a_dispute() {
    let r = rig();
    let s = host();
    let x = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("x"), FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
    // The owner's value and the observed one disagree only if the owner fact was written
    // around the service; build that state through two sources instead.
    let y = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("y"), FactSource::Integration),
            &system(),
        )
        .await
        .unwrap();
    assert_eq!(y.fact.value.status, KnowledgeStatus::Disputed);
    // The integration changes its mind and now agrees with the discovery.
    let agree = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("x"), FactSource::Integration),
            &system(),
        )
        .await
        .unwrap();
    let live = r.store.live_facts(&s, Predicate::Role).await.unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].value.id, x.fact.value.id);
    assert_eq!(live[0].value.status, KnowledgeStatus::Current);
    assert_eq!(agree.fact.value.id, x.fact.value.id);
}

#[tokio::test]
async fn repeated_record_of_the_same_value_refreshes_without_a_new_row() {
    let r = rig();
    let s = host();
    let first = r
        .service
        .record(
            draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
    r.clock.advance(SignedDuration::from_secs(DAY));
    let mut again = draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered);
    again.observed_at = Some(r.clock_now());
    let second = r.service.record(again, &system()).await.unwrap();
    assert_eq!(second.effect, RecordEffect::Refreshed);
    assert_eq!(second.fact.value.id, first.fact.value.id);
    assert_eq!(second.fact.version, first.fact.version + 1);
    assert_eq!(second.fact.value.last_verified_at, Some(r.clock_now()));
    assert_eq!(second.fact.value.observed_at, r.clock_now());
    assert_eq!(r.store.all_live_facts(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn many_predicates_deduplicate_equal_values() {
    let r = rig();
    let s = host();
    let a = owner_fact(&r, &s, Predicate::OwnerNote, json!("same")).await;
    let b = owner_fact(&r, &s, Predicate::OwnerNote, json!("same")).await;
    assert_eq!(a.fact.value.id, b.fact.value.id);
    owner_fact(&r, &s, Predicate::OwnerNote, json!("different")).await;
    let live = r.store.live_facts(&s, Predicate::OwnerNote).await.unwrap();
    assert_eq!(live.len(), 2);
    assert!(
        live.iter()
            .all(|v| v.value.status == KnowledgeStatus::Current)
    );
}

#[tokio::test]
async fn stale_fact_is_marked_by_sweep_and_labelled_on_read() {
    let r = rig();
    let s = host();
    let rec = owner_fact(&r, &s, Predicate::Role, json!("nas")).await;
    r.clock.advance(SignedDuration::from_secs(181 * DAY));
    let read = r.store.fact(rec.fact.value.id).await.unwrap().unwrap();
    assert_eq!(read.value.status, KnowledgeStatus::Current);
    assert_eq!(
        read.value.effective_status(r.clock_now()),
        KnowledgeStatus::Stale
    );
    let report = r.service.sweep().await.unwrap();
    assert_eq!(report.marked_stale, 1);
    let read = r.store.fact(rec.fact.value.id).await.unwrap().unwrap();
    assert_eq!(read.value.status, KnowledgeStatus::Stale);
    let entries = r.store.entries_after(0, 100).await.unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.kind == "fact.changed" && e.payload["change"] == "stale")
    );
    assert_eq!(r.service.sweep().await.unwrap().marked_stale, 0);
}

#[tokio::test]
async fn refresh_revives_a_stale_ordinary_fact() {
    let r = rig();
    let s = host();
    r.service
        .record(
            draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
    r.clock.advance(SignedDuration::from_secs(181 * DAY));
    r.service.sweep().await.unwrap();
    let mut again = draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered);
    again.observed_at = Some(r.clock_now());
    let rec = r.service.record(again, &system()).await.unwrap();
    assert_eq!(rec.fact.value.status, KnowledgeStatus::Current);
}

async fn stale_owner_path(r: &Rig, s: &SubjectRef) -> Recorded {
    let rec = owner_fact(
        r,
        s,
        Predicate::ManagementPath,
        json!({"cidr": "192.168.1.0/24"}),
    )
    .await;
    r.clock.advance(SignedDuration::from_secs(31 * DAY));
    r.service.sweep().await.unwrap();
    rec
}

#[tokio::test]
async fn reasserting_a_stale_management_path_does_not_make_it_current() {
    let r = rig();
    let s = host();
    let first = stale_owner_path(&r, &s).await;
    let again = r
        .service
        .assert_fact(
            draft(
                &s,
                Predicate::ManagementPath,
                json!({"cidr": "192.168.1.0/24"}),
                FactSource::User,
            ),
            UserId::new(),
        )
        .await
        .unwrap();
    assert_eq!(again.effect, RecordEffect::Unchanged);
    assert_eq!(again.fact.value.id, first.fact.value.id);
    assert_eq!(again.fact.value.status, KnowledgeStatus::Stale);
    assert_eq!(again.fact.value.last_verified_at, None);
}

#[tokio::test]
async fn observation_cannot_revive_a_stale_management_path() {
    let r = rig();
    let s = host();
    stale_owner_path(&r, &s).await;
    let seen = r
        .service
        .record(
            draft(
                &s,
                Predicate::ManagementPath,
                json!({"cidr": "192.168.1.0/24"}),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    assert_eq!(seen.effect, RecordEffect::Unchanged);
    assert_eq!(seen.fact.value.status, KnowledgeStatus::Stale);
}

#[tokio::test]
async fn verify_refuses_a_management_path_and_revives_a_trusted_cidr() {
    let r = rig();
    let s = host();
    let path = owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "10.0.0.0/8"}),
    )
    .await;
    let err = r
        .service
        .verify(path.fact.value.id, path.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(err, KnowledgeError::UseConfirmation);
    let cidr = owner_fact(
        &r,
        &s,
        Predicate::TrustedCidr,
        json!({"cidr": "10.0.0.0/8"}),
    )
    .await;
    r.clock.advance(SignedDuration::from_secs(31 * DAY));
    r.service.sweep().await.unwrap();
    let stale = r.store.fact(cidr.fact.value.id).await.unwrap().unwrap();
    assert_eq!(stale.value.status, KnowledgeStatus::Stale);
    let verified = r
        .service
        .verify(stale.value.id, stale.version, UserId::new())
        .await
        .unwrap();
    assert_eq!(verified.value.status, KnowledgeStatus::Current);
    assert!(verified.value.usable_for_safety(r.clock_now()));
}

#[tokio::test]
async fn management_path_values_are_canonical_and_deduplicated() {
    let r = rig();
    let s = host();
    owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "192.168.1.7/24"}),
    )
    .await;
    owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "192.168.1.0/24"}),
    )
    .await;
    let live = r
        .store
        .live_facts(&s, Predicate::ManagementPath)
        .await
        .unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].value.value, Some(json!({"cidr": "192.168.1.0/24"})));
}

#[tokio::test]
async fn safety_critical_facts_ignore_non_user_sources() {
    let r = rig();
    let s = host();
    let SubjectRef::Host(h) = s else {
        unreachable!()
    };
    for p in [Predicate::ManagementPath, Predicate::TrustedCidr] {
        let rec = r
            .service
            .record(
                draft(&s, p, json!({"cidr": "10.0.0.0/8"}), FactSource::Discovered),
                &system(),
            )
            .await
            .unwrap();
        assert!(!rec.fact.value.usable_for_safety(r.clock_now()));
    }
    let paths = r.service.management_paths(h).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0].state,
        domovoi_core::knowledge::PathState::Candidate
    );
}

#[tokio::test]
async fn host_platform_accepts_only_observation_sources() {
    let r = rig();
    let s = host();
    let v = json!({"hostname": "n", "os": "d", "kernel": "k", "arch": "x", "cpu_count": 4, "mem_total_bytes": 8});
    for source in [FactSource::Inferred, FactSource::Imported] {
        let err = r
            .service
            .record(
                draft(&s, Predicate::HostPlatform, v.clone(), source),
                &system(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, KnowledgeError::InvalidInput(_)), "{source:?}");
    }
    r.service
        .record(
            draft(&s, Predicate::HostPlatform, v, FactSource::Discovered),
            &system(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn journal_and_audit_never_contain_fact_values() {
    let r = rig();
    let s = host();
    let a = owner_fact(&r, &s, Predicate::Role, json!("zebra-quartz-role")).await;
    let b = r
        .service
        .record(
            draft(
                &s,
                Predicate::OwnerNote,
                json!("walrus-ember-note"),
                FactSource::Inferred,
            ),
            &system(),
        )
        .await
        .unwrap();
    r.service
        .accept(b.fact.value.id, b.fact.version, UserId::new())
        .await
        .unwrap();
    let p = owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "172.16.9.0/24"}),
    )
    .await;
    r.coverage.cover(&["172.16.9.0/24"]);
    r.service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap();
    r.service
        .retract(a.fact.value.id, a.fact.version, UserId::new())
        .await
        .unwrap();
    let mut text = String::new();
    for e in r.store.entries_after(0, 500).await.unwrap() {
        text.push_str(&e.payload.to_string());
    }
    for e in all_audit(&r).await {
        text.push_str(&format!("{} {} {}", e.subject, e.kind, e.detail));
    }
    for secret in ["zebra-quartz-role", "walrus-ember-note", "172.16.9.0"] {
        assert!(!text.contains(secret), "{secret} leaked");
    }
    assert!(text.contains("role"));
}

#[tokio::test]
async fn stale_version_changes_nothing() {
    let r = rig();
    let s = host();
    let rec = owner_fact(&r, &s, Predicate::Role, json!("nas")).await;
    let (journal, audits) = (journal_len(&r).await, all_audit(&r).await.len());
    let err = r
        .service
        .retract(rec.fact.value.id, rec.fact.version + 5, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(err, KnowledgeError::Conflict);
    let after = r.store.fact(rec.fact.value.id).await.unwrap().unwrap();
    assert_eq!(after.value.status, KnowledgeStatus::Current);
    assert_eq!(
        (journal_len(&r).await, all_audit(&r).await.len()),
        (journal, audits)
    );
}

#[tokio::test]
async fn retract_works_on_every_predicate_and_is_audited() {
    let r = rig();
    let user = UserId::new();
    let host_ref = host();
    let container = SubjectRef::Container {
        host: host_ref.host().unwrap(),
        name: "plex".into(),
    };
    let cases = vec![
        (host_ref.clone(), Predicate::Role, json!("a")),
        (host_ref.clone(), Predicate::Criticality, json!("critical")),
        (host_ref.clone(), Predicate::OwnerNote, json!("a")),
        (
            host_ref.clone(),
            Predicate::DependsOn,
            json!(host().to_string()),
        ),
        (
            host_ref.clone(),
            Predicate::ManagementPath,
            json!({"cidr": "10.0.0.0/8"}),
        ),
        (
            host_ref.clone(),
            Predicate::MaintenanceWindow,
            json!({"starts_at": "2027-01-01T00:00:00Z", "ends_at": "2027-01-01T02:00:00Z"}),
        ),
        (host_ref.clone(), Predicate::BackupLocation, json!("nas")),
        (container.clone(), Predicate::NeverRestart, json!(true)),
        (
            host_ref.clone(),
            Predicate::TrustedCidr,
            json!({"cidr": "10.0.0.0/8"}),
        ),
    ];
    let count = cases.len();
    for (s, p, v) in cases {
        let rec = owner_fact(&r, &s, p, v).await;
        let gone = r
            .service
            .retract(rec.fact.value.id, rec.fact.version, user)
            .await
            .unwrap();
        assert_eq!(gone.value.status, KnowledgeStatus::Retracted, "{p:?}");
        assert_eq!(gone.value.retracted_by, Some(Actor::User(user)));
    }
    assert_eq!(audit(&r, "knowledge.fact.retracted").await.len(), count);
}
