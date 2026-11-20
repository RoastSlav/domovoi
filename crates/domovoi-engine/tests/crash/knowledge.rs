//! The knowledge service crashed at every commit of a short history: each call is one atomic
//! commit, so a crash leaves either all of its change, journal and audit entries or none, and
//! repeating the history after a restart ends with one live owner fact.

use std::sync::Arc;

use domovoi_core::knowledge::{FactSource, KnowledgeStatus, Predicate, SubjectRef};
use domovoi_core::ports::{AuditFilter, AuditLog, JournalRead, KnowledgeRead, Page};
use domovoi_core::{Actor, HostId, UserId};
use domovoi_engine::knowledge::{FactDraft, KnowledgeDeps, KnowledgeError, KnowledgeService};
use domovoi_testkit::build::T0;
use domovoi_testkit::crash::{CrashPlan, CrashPoint, CrashingStore};
use domovoi_testkit::{FakeClock, FakeCoverage, InMemoryStore};
use serde_json::json;

fn draft(subject: &SubjectRef, value: &str, source: FactSource) -> FactDraft {
    FactDraft {
        subject: subject.clone(),
        predicate: Predicate::Role,
        value: json!(value),
        source,
        observed_at: None,
        valid_until: None,
        evidence: None,
    }
}

async fn history(
    service: &KnowledgeService,
    subject: &SubjectRef,
    user: UserId,
) -> Result<(), KnowledgeError> {
    let system = Actor::System("knowledge".to_owned());
    service
        .record(draft(subject, "nas", FactSource::Discovered), &system)
        .await?;
    service
        .record(draft(subject, "backup", FactSource::Integration), &system)
        .await?;
    service
        .assert_fact(draft(subject, "nas", FactSource::User), user)
        .await?;
    Ok(())
}

#[tokio::test]
async fn knowledge_survives_a_crash_at_every_commit_in_memory() {
    for at_commit in 1..=3 {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let store = Arc::new(InMemoryStore::new());
            let crashing = Arc::new(CrashingStore::new(store.clone()));
            let service = KnowledgeService::new(KnowledgeDeps {
                uow: crashing.clone(),
                read: store.clone(),
                coverage: Arc::new(FakeCoverage::unavailable()),
                clock: Arc::new(FakeClock::new(T0)),
            });
            let subject = SubjectRef::Host(HostId::new());
            let user = UserId::new();
            crashing.arm(CrashPlan { at_commit, point });
            assert!(
                history(&service, &subject, user).await.is_err(),
                "commit {at_commit}, {point:?}: the crash never happened"
            );
            crashing.restart();
            history(&service, &subject, user).await.unwrap();

            let live = store.live_facts(&subject, Predicate::Role).await.unwrap();
            assert_eq!(live.len(), 1, "commit {at_commit}, {point:?}");
            assert_eq!(live[0].value.source, FactSource::User);
            assert_eq!(live[0].value.status, KnowledgeStatus::Current);
            let asserted = store
                .list(
                    &AuditFilter {
                        kind: Some("knowledge.fact.asserted".to_owned()),
                        ..AuditFilter::default()
                    },
                    &Page::first(50),
                )
                .await
                .unwrap();
            // A crash after the third commit means the repeat asserts the value again, which
            // is a second owner action; before it, the repeat is the first.
            assert!(
                (1..=2).contains(&asserted.items.len()),
                "commit {at_commit}, {point:?}: an audit entry for each owner assertion that committed"
            );
            for event in &asserted.items {
                let id = event.subject.trim_start_matches("fact:").parse().unwrap();
                assert!(
                    store.fact(id).await.unwrap().is_some(),
                    "commit {at_commit}, {point:?}: an audit entry names a fact that does not exist"
                );
            }
            for entry in store.entries_after(0, 500).await.unwrap() {
                let id = entry.aggregate.id.parse().unwrap();
                assert!(
                    store.fact(id).await.unwrap().is_some(),
                    "commit {at_commit}, {point:?}: a journal entry names a fact that does not exist"
                );
            }
        }
    }
}
