//! The action-status metric counts changes, not writes: creating an action and a real
//! transition increment it, while a chat-token rotation or a refused commit does not.
//!
//! The recorder is installed for the current thread only, and every test runs on a
//! single-threaded runtime, so the counts never leak between tests.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::ActionRepo;
use domovoi_core::{ActionError, ActionRequest, Risk, UserId};
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_engine::{ActionSettings, ApprovalError};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use metrics::{
    Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};

use crate::actions_harness::{actions, seeded, verified_restart};
use crate::harness::host_id;
use crate::uow_probe::{RefusingAuditUow, StaleRaceUow};

/// Counts `domovoi_actions_total` increments by their `status` label.
#[derive(Default)]
struct ActionMetricsRecorder {
    counts: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl ActionMetricsRecorder {
    fn count(&self, status: &str) -> u64 {
        self.counts().get(status).copied().unwrap_or_default()
    }

    fn counts(&self) -> BTreeMap<String, u64> {
        self.counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn total(&self) -> u64 {
        self.counts().values().sum()
    }
}

struct StatusCounter {
    status: String,
    counts: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl CounterFn for StatusCounter {
    fn increment(&self, value: u64) {
        *self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(self.status.clone())
            .or_default() += value;
    }

    fn absolute(&self, value: u64) {
        self.counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(self.status.clone(), value);
    }
}

impl Recorder for ActionMetricsRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        if key.name() != domovoi_engine::telemetry::ACTIONS_TOTAL {
            return Counter::noop();
        }
        let status = key
            .labels()
            .find(|label| label.key() == "status")
            .map(|label| label.value().to_owned());
        match status {
            Some(status) => Counter::from_arc(Arc::new(StatusCounter {
                status,
                counts: self.counts.clone(),
            })),
            None => Counter::noop(),
        }
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

/// A proposed `restart_container` action, stored directly so a failing commit is the only
/// thing a test measures.
fn proposed(incident: domovoi_core::IncidentId) -> ActionRequest {
    domovoi_testkit::build::action(incident, host_id(), Risk::Write).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn creating_an_action_increments_the_proposed_counter() {
    let recorder = ActionMetricsRecorder::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a
        .proposal(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;

    a.service.propose(action).await.unwrap();

    assert_eq!(
        recorder.count("proposed"),
        1,
        "a creation is a change from no status to proposed",
    );
    assert_eq!(recorder.total(), 1, "no other status was recorded");
}

#[tokio::test(flavor = "current_thread")]
async fn a_status_transition_increments_the_destination_counter() {
    let recorder = ActionMetricsRecorder::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;

    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();

    assert_eq!(
        recorder.count("approved"),
        1,
        "the transition is counted for the status it reached",
    );
    assert_eq!(
        recorder.count("proposed"),
        0,
        "the seeded proposal did not write a metric",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rotating_a_chat_token_does_not_increment_any_action_status_counter() {
    let recorder = ActionMetricsRecorder::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a
        .proposal(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;
    let stored = a.service.propose(action).await.unwrap();
    assert_eq!(recorder.count("proposed"), 1, "the creation was counted");
    let before = recorder.counts();

    let new_token = mint_token();
    a.service
        .rotate_chat_token(stored.id(), token_hash(&new_token))
        .await
        .unwrap();

    assert_eq!(
        recorder.counts(),
        before,
        "rotating a chat token is a same-status update, not a transition",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_commit_does_not_increment_the_counter() {
    let recorder = ActionMetricsRecorder::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let world = seeded().await;
    let store = world.store.clone();
    let action = proposed(world.incident.id());
    let id = action.id();
    ActionRepo::insert(&*store, &action).await.unwrap();
    let a = world.start(
        ActionSettings::default(),
        Arc::new(RefusingAuditUow::new(store)),
    );

    let refused = a.service.approve(id, human(UserId::new()), None).await;

    assert!(
        matches!(refused, Err(ApprovalError::Store(_))),
        "the audit failure refused the transition: {refused:?}",
    );
    assert_eq!(recorder.total(), 0, "a refused commit records nothing");
}

#[tokio::test(flavor = "current_thread")]
async fn a_conflicted_commit_does_not_increment_the_counter() {
    let recorder = ActionMetricsRecorder::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let world = seeded().await;
    let action = proposed(world.incident.id());
    let mut rival = action.clone();
    rival.deny(T0).unwrap();
    ActionRepo::insert(&*world.store, &action).await.unwrap();
    let a = world.start(
        ActionSettings::default(),
        Arc::new(StaleRaceUow::new(world.store.clone(), rival)),
    );

    let refused = a
        .service
        .approve(action.id(), human(UserId::new()), None)
        .await;

    assert_eq!(
        refused.err(),
        Some(ApprovalError::Refused(ActionError::ChangedMeanwhile)),
        "the writer lost the race",
    );
    assert_eq!(recorder.total(), 0, "a conflicted commit records nothing");
}
