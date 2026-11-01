//! Whatever arrives, a fingerprint never has two active incidents.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{IncidentFilter, IncidentRepo, Page};
use domovoi_core::{Fingerprint, HostId};
use domovoi_engine::{EnginePolicy, IncidentManager, ManagerDeps, PatternRedactor};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::SignedDuration;
use proptest::prelude::*;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::harness::Sig;

#[derive(Debug, Clone)]
enum Step {
    Fire(usize, bool),
    Resolve(usize),
    Tick,
    Wait(i64),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0usize..3, any::<bool>()).prop_map(|(n, immediate)| Step::Fire(n, immediate)),
        3 => (0usize..3).prop_map(Step::Resolve),
        1 => Just(Step::Tick),
        2 => (1i64..20_000).prop_map(Step::Wait),
    ]
}

async fn run_steps(steps: Vec<Step>) {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, _events) = broadcast::channel(1024);
    let mut policy = EnginePolicy::default();
    policy.incident.ttl = Some(SignedDuration::from_hours(1));
    let shutdown = CancellationToken::new();
    let (handle, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy,
        bus,
        shutdown: shutdown.clone(),
    });
    let host = HostId::new();
    let sigs = [
        Sig::new(host, "disk.free", Some("/")),
        Sig::new(host, "disk.free", Some("/srv")),
        Sig::new(host, "unit.failed", Some("backup.service")),
    ];
    let mut secs = 0;
    for step in steps {
        let result = match step {
            Step::Fire(n, immediate) => {
                let sig = if immediate {
                    sigs[n].clone().immediate()
                } else {
                    sigs[n].clone()
                };
                handle.ingest(vec![sig.firing(secs)]).await
            }
            Step::Resolve(n) => handle.ingest(vec![sigs[n].resolved(secs)]).await,
            Step::Tick => handle.tick().await,
            Step::Wait(by) => {
                secs += by;
                clock.advance(SignedDuration::from_secs(by));
                Ok(())
            }
        };
        assert!(result.is_ok(), "{result:?}");
        let filter = IncidentFilter {
            active: Some(true),
            ..IncidentFilter::default()
        };
        let active = store.list(&filter, &Page::first(100)).await.unwrap().items;
        let mut per_fingerprint: BTreeMap<Fingerprint, usize> = BTreeMap::new();
        for incident in &active {
            *per_fingerprint.entry(incident.fingerprint()).or_default() += 1;
        }
        assert!(per_fingerprint.values().all(|n| *n == 1), "{active:#?}");
    }
    shutdown.cancel();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn at_most_one_active_incident_per_fingerprint(steps in proptest::collection::vec(step(), 0..40)) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(run_steps(steps));
    }
}
