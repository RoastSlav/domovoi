//! The host profile's machine description as a `host_platform` fact, and the move of old
//! generated notes out of the user's notes.

use std::sync::Arc;

use crate::api_harness::{Api, api};
use crate::profile::{handshake, profile_deps, seed};
use domovoi_core::Actor;
use domovoi_core::knowledge::{Fact, FactSource, KnowledgeStatus, Predicate, SubjectRef};
use domovoi_core::ports::{FactFilter, KnowledgeRead, NoteRepo, Page, SettingsRepo};
use domovoi_engine::knowledge::{KnowledgeDeps, KnowledgeService, UnavailableCoverage};
use domovoi_hub::profile::backfill::backfill;
use domovoi_hub::profile::{ProfileDeps, migrate, refresh};
use serde_json::json;

fn service(a: &Api) -> Arc<KnowledgeService> {
    Arc::new(KnowledgeService::new(KnowledgeDeps {
        uow: a.store.clone(),
        read: a.store.clone(),
        coverage: Arc::new(UnavailableCoverage),
        clock: a.clock.clone(),
    }))
}

fn with_knowledge(a: &Api, knowledge: &Arc<KnowledgeService>) -> ProfileDeps {
    ProfileDeps {
        knowledge: Some(knowledge.clone()),
        ..profile_deps(a)
    }
}

/// Every platform fact of the harness host, any status.
async fn platform_facts(a: &Api, knowledge: &KnowledgeService) -> Vec<Fact> {
    let filter = FactFilter {
        subject: Some(SubjectRef::Host(a.nas.id)),
        predicate: Some(Predicate::HostPlatform),
        statuses: Some(vec![
            KnowledgeStatus::Current,
            KnowledgeStatus::Stale,
            KnowledgeStatus::Disputed,
            KnowledgeStatus::Superseded,
        ]),
        ..FactFilter::default()
    };
    knowledge
        .facts(&filter, &Page::first(50))
        .await
        .map(|page| page.items)
        .unwrap_or_default()
}

#[tokio::test]
async fn backfill_writes_a_discovered_host_platform_fact() {
    let a = api().await;
    seed(&a, "0.6.0");
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let knowledge = service(&a);
    backfill(&with_knowledge(&a, &knowledge)).await;
    let facts = platform_facts(&a, &knowledge).await;
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    assert_eq!(fact.source, FactSource::Discovered);
    assert_eq!(fact.actor, Actor::System("profile".to_owned()));
    assert!(fact.evidence.is_none(), "no note stands behind it any more");
    assert_eq!(fact.observed_at, profile.collected_at);
    let f = &profile.facts;
    assert_eq!(
        fact.value,
        Some(json!({
            "hostname": f.hostname, "os": f.os, "kernel": f.kernel, "arch": f.arch,
            "cpu_count": f.cpu_count, "mem_total_bytes": f.mem_total_bytes,
        }))
    );
}

#[tokio::test]
async fn backfill_is_idempotent() {
    let a = api().await;
    seed(&a, "0.6.0");
    refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let knowledge = service(&a);
    let deps = with_knowledge(&a, &knowledge);
    backfill(&deps).await;
    let first = knowledge
        .facts(&FactFilter::default(), &Page::first(50))
        .await
        .unwrap()
        .items;
    let before = a.store.fact(first[0].id).await.unwrap().unwrap().version;
    backfill(&deps).await;
    let facts = platform_facts(&a, &knowledge).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].id, first[0].id);
    assert_eq!(
        a.store.fact(first[0].id).await.unwrap().unwrap().version,
        before
    );
}

#[tokio::test]
async fn a_profile_is_never_written_as_a_note_and_old_generated_notes_are_removed() {
    use domovoi_core::Note;
    let a = api().await;
    seed(&a, "0.6.0");
    // A note an older hub wrote, and one the person wrote about the same host.
    let old = Note {
        id: domovoi_core::NoteId::new(),
        host: Some(a.nas.id),
        title: "Host profile: nas".to_owned(),
        body_markdown: "# Host profile: nas".to_owned(),
        tags: vec!["host-profile".to_owned(), "generated".to_owned()],
        updated_at: a.now(),
    };
    let mine = Note {
        id: domovoi_core::NoteId::new(),
        title: "Backups run at 02:00".to_owned(),
        tags: vec!["backup".to_owned()],
        ..old.clone()
    };
    NoteRepo::upsert(&*a.store, &old).await.unwrap();
    NoteRepo::upsert(&*a.store, &mine).await.unwrap();
    let deps = profile_deps(&a);
    refresh(&deps, &a.nas).await.unwrap();
    migrate(&deps).await;
    migrate(&deps).await;
    let notes = NoteRepo::list(&*a.store, Some(a.nas.id)).await.unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].id, mine.id, "the person's note stays");
}

#[tokio::test]
async fn migrate_stores_the_text_triage_reads_for_a_profile_made_before() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let profile = refresh(&deps, &a.nas).await.unwrap();
    let key = domovoi_core::profile_text_key(a.nas.id);
    a.store.set_many(&[(key.clone(), None)]).await.unwrap();
    assert!(SettingsRepo::get(&*a.store, &key).await.unwrap().is_none());
    migrate(&deps).await;
    let stored = SettingsRepo::get(&*a.store, &key)
        .await
        .unwrap()
        .expect("the text is back");
    assert_eq!(
        stored["markdown"],
        domovoi_hub::profile::note_text(&a.nas, &profile)
    );
}

#[tokio::test]
async fn profile_refresh_records_the_platform_fact_and_an_unchanged_refresh_adds_no_row() {
    let a = api().await;
    seed(&a, "0.6.0");
    let knowledge = service(&a);
    let deps = with_knowledge(&a, &knowledge);
    refresh(&deps, &a.nas).await.unwrap();
    assert_eq!(platform_facts(&a, &knowledge).await.len(), 1);
    a.clock.advance(jiff::SignedDuration::from_mins(10));
    refresh(&deps, &a.nas).await.unwrap();
    let facts = platform_facts(&a, &knowledge).await;
    assert_eq!(facts.len(), 1);
    assert!(facts[0].last_verified_at.is_some());
    assert_eq!(facts[0].status, KnowledgeStatus::Current);
}

#[tokio::test]
async fn a_changed_kernel_supersedes_the_platform_fact() {
    let a = api().await;
    seed(&a, "0.6.0");
    let knowledge = service(&a);
    let deps = with_knowledge(&a, &knowledge);
    refresh(&deps, &a.nas).await.unwrap();
    a.clock.advance(jiff::SignedDuration::from_mins(10));
    let mut facts = handshake(a.now());
    facts.kernel = "6.13.1".to_owned();
    a.link.set_facts(a.nas.id, facts);
    refresh(&deps, &a.nas).await.unwrap();
    let all = platform_facts(&a, &knowledge).await;
    assert_eq!(all.len(), 2);
    let current: Vec<_> = all
        .iter()
        .filter(|f| f.status == KnowledgeStatus::Current)
        .collect();
    let old: Vec<_> = all
        .iter()
        .filter(|f| f.status == KnowledgeStatus::Superseded)
        .collect();
    assert_eq!((current.len(), old.len()), (1, 1));
    assert_eq!(current[0].supersedes, Some(old[0].id));
    assert_eq!(current[0].value.as_ref().unwrap()["kernel"], "6.13.1");
}

#[tokio::test]
async fn profile_refresh_succeeds_when_knowledge_is_not_configured() {
    let a = api().await;
    seed(&a, "0.6.0");
    let knowledge = service(&a);
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert!(!profile.sections.is_empty());
    assert!(platform_facts(&a, &knowledge).await.is_empty());
}
