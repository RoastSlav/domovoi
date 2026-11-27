//! What `prepare` reports before a model call: a cached report, a refusal or admission.

use std::sync::Arc;

use domovoi_core::ports::{
    AdmissionOutcome, AdmissionPurpose, AdmissionRepo, Clock, IncidentRepo, LlmProvider,
    StoreError, TriageCacheRepo, UsageRepo,
};
use domovoi_core::{HostId, Incident, TriageReport, Verdict};
use domovoi_testkit::build::{T0, open_incident, resolved_incident};
use domovoi_testkit::link::{evidence_caps, observer_of};
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, InMemoryStore, ScriptedLlm,
};
use jiff::SignedDuration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::admission::{Refusal, WINDOW, slot_after};
use super::run::{Limits, Prepared, digest, prepare};
use super::{Shared, TriageDeps, TriageSettings};
use crate::evidence::{Evidence, HostCatalog};
use crate::manager::{IncidentManager, ManagerDeps};
use crate::policy::EnginePolicy;
use crate::redact::PatternRedactor;

/// How the store is primed before `prepare` runs.
#[derive(Clone, Copy, Debug)]
enum Setup {
    CachedFromResolvedTwin,
    SpentBudget,
    FullHour,
    UnreadableWindow,
    OpenHour,
}

/// What `prepare` reported, without the report text.
#[derive(Debug, PartialEq, Eq)]
enum Reported {
    Cached,
    Refused(Refusal),
    Admitted,
}

fn host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

fn report() -> TriageReport {
    TriageReport {
        headline: "nginx keeps restarting".to_owned(),
        summary: "It crashed three times.".to_owned(),
        cause: "A bad config.".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::ActionNeeded,
        next_step: "Fix the config.".to_owned(),
        manual_steps: Vec::new(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

fn shared_of(deps: TriageDeps, settings: TriageSettings) -> Shared {
    Shared {
        deps,
        settings,
        limits: std::sync::Mutex::new(Limits::default()),
        questions: tokio::sync::Semaphore::new(1),
    }
}

async fn run(setup: Setup) -> Reported {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    let incident: Incident = open_incident(host(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let (bus, _events) = broadcast::channel(16);
    let shutdown = CancellationToken::new();
    let redactor = Arc::new(PatternRedactor::new([]));
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: redactor.clone(),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let capabilities = Arc::new(FakeCapabilityRegistry::following(link.clone()));
    let primary: Arc<dyn LlmProvider> = Arc::new(ScriptedLlm::new());
    let deps = TriageDeps {
        primary,
        fallbacks: Vec::new(),
        evidence: Evidence::new(
            evidence_caps(link.clone()),
            clock.clone(),
            capabilities.clone(),
        ),
        catalog: HostCatalog::new(observer_of(link.clone())),
        capabilities,
        resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
        incidents: store.clone(),
        acks: store.clone(),
        signals: store.clone(),
        notes: store.clone(),
        hosts: store.clone(),
        cache: store.clone(),
        usage: store.clone(),
        admission: store.clone(),
        advisories: store.clone(),
        redactor: redactor.clone(),
        clock: clock.clone(),
        manager,
        bus,
        shutdown: shutdown.clone(),
        experience: crate::context::ExperienceSource::over(
            store.clone(),
            redactor.clone(),
            clock.clone(),
        ),
        extra_context: Vec::new(),
    };
    let mut settings = TriageSettings::default();
    let mut fresh = true;
    match setup {
        Setup::CachedFromResolvedTwin => {
            fresh = false;
            let twin_resolved = clock.now() - SignedDuration::from_mins(10);
            let twin = resolved_incident(host(), "docker.container", Some("nginx"), twin_resolved)
                .unwrap();
            IncidentRepo::insert(&*store, &twin).await.unwrap();
            let shared = shared_of(deps.clone(), settings);
            let evidence = digest(&shared, &incident).await;
            TriageCacheRepo::put(&*store, &incident.fingerprint(), &evidence, &report(), T0)
                .await
                .unwrap();
        }
        Setup::SpentBudget => {
            settings.monthly_budget_micros = Some(100);
            UsageRepo::record(&*store, "scripted", 10, 10, 100, T0)
                .await
                .unwrap();
        }
        Setup::FullHour => {
            settings.per_hour_runs = 1;
            let outcome = AdmissionRepo::admit(&*store, AdmissionPurpose::Triage, 1, WINDOW, T0)
                .await
                .unwrap();
            assert!(matches!(outcome, AdmissionOutcome::Admitted));
        }
        Setup::UnreadableWindow => {
            store.fail_next(StoreError::Backend("window unreadable".to_owned()));
        }
        Setup::OpenHour => {}
    }
    let shared = shared_of(deps, settings);
    let reported = match prepare(&shared, &incident, fresh, false).await {
        Prepared::Cached { from, .. } => {
            assert_ne!(
                from,
                incident.id(),
                "the report came from the resolved twin"
            );
            Reported::Cached
        }
        Prepared::Refused(refusal) => Reported::Refused(refusal),
        Prepared::Admitted => Reported::Admitted,
    };
    shutdown.cancel();
    reported
}

#[tokio::test]
async fn prepare_reports_cached_refused_and_admitted() {
    let table = [
        (Setup::CachedFromResolvedTwin, Reported::Cached),
        (Setup::SpentBudget, Reported::Refused(Refusal::BudgetSpent)),
        (
            Setup::FullHour,
            Reported::Refused(Refusal::Hourly {
                retry_at: slot_after(T0),
            }),
        ),
        (
            Setup::UnreadableWindow,
            Reported::Refused(Refusal::Unverifiable),
        ),
        (Setup::OpenHour, Reported::Admitted),
    ];
    for (setup, expected) in table {
        assert_eq!(run(setup).await, expected, "{setup:?}");
    }
}
