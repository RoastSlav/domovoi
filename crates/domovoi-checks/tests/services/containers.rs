//! Container baselines, criticality, restart loops and new containers.

use std::collections::BTreeSet;
use std::sync::Arc;

use domovoi_checks::containers::{
    ContainerEvent, ContainerPolicy, EventLog, ExpectedContainers, Health, NewContainers,
    RestartLoop, UnhealthyContainers,
};
use domovoi_core::{FactValue, Severity};
use jiff::SignedDuration;

use crate::harness::{
    FakeDocker, Harness, assert_firing, assert_ok, container, deps, find, firing_targets,
    is_immediate, number, stopped, targets, text,
};

const MINUTE: SignedDuration = SignedDuration::from_mins(1);

#[tokio::test]
async fn managed_container_is_learned_after_two_runs() {
    let docker = Arc::new(FakeDocker::default());
    docker.set(vec![container("plex", true)]);
    let check = ExpectedContainers::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;

    let first = h.run(&check).await;
    assert!(first.observations.is_empty(), "{first:#?}");

    h.advance(MINUTE);
    let second = h.run(&check).await;
    assert_ok(&second, "plex");

    docker.set(Vec::new());
    h.advance(MINUTE);
    let gone = h.run(&check).await;
    assert_firing(&gone, "plex", Severity::Warning);
    assert_eq!(text(find(&gone, "plex"), "state"), "missing");
}

#[tokio::test]
async fn auto_remove_and_unmanaged_containers_are_never_learned() {
    let docker = Arc::new(FakeDocker::default());
    let mut throwaway = container("migrate", true);
    throwaway.auto_remove = true;
    docker.set(vec![throwaway, container("adhoc", false)]);
    let check = ExpectedContainers::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    for _ in 0..3 {
        assert!(h.run(&check).await.observations.is_empty());
        h.advance(MINUTE);
    }
    docker.set(Vec::new());
    let after = h.run(&check).await;
    assert!(after.observations.is_empty(), "{after:#?}");
}

#[tokio::test]
async fn expected_container_down_uses_configured_criticality() {
    let docker = Arc::new(FakeDocker::default());
    docker.set(vec![container("db", true), container("web", true)]);
    let policy = ContainerPolicy {
        critical: BTreeSet::from(["db".to_owned()]),
        ..ContainerPolicy::default()
    };
    let check = ExpectedContainers::new(deps(&docker, policy)).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.run(&check).await;
    h.advance(MINUTE);
    let up = h.run(&check).await;
    let fact = up
        .facts
        .iter()
        .find(|f| f.key == "containers.expected_running")
        .unwrap();
    assert_eq!(
        fact.value,
        FactValue::Text("2/2 expected containers running".to_owned())
    );
    assert!(
        up.facts
            .iter()
            .any(|f| f.key == "container.db.image_age_days")
    );

    docker.set(vec![
        stopped(container("db", true)),
        stopped(container("web", true)),
    ]);
    h.advance(MINUTE);
    let down = h.run(&check).await;
    assert_firing(&down, "db", Severity::Critical);
    assert_firing(&down, "web", Severity::Warning);
    assert_eq!(text(find(&down, "db"), "state"), "exited");
}

#[tokio::test]
async fn restart_loop_fires_on_three_restarts_within_an_hour() {
    let docker = Arc::new(FakeDocker::default());
    let check = RestartLoop::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    let restarts = |count: u64| {
        let mut c = container("worker", true);
        c.restart_count = count;
        docker.set(vec![c]);
    };

    restarts(5);
    assert_ok(&h.run(&check).await, "worker");
    h.advance(SignedDuration::from_mins(10));
    restarts(7);
    assert_ok(&h.run(&check).await, "worker");
    h.advance(SignedDuration::from_mins(10));
    restarts(8);
    let looping = h.run(&check).await;
    assert_firing(&looping, "worker", Severity::Warning);
    assert_eq!(number(find(&looping, "worker"), "restarts"), 3.0);
    assert_eq!(number(find(&looping, "worker"), "restart_count"), 8.0);

    h.advance(SignedDuration::from_hours(2));
    assert_ok(&h.run(&check).await, "worker");

    // Slow restarts spread over hours never add up to a loop.
    for count in 9..12 {
        h.advance(SignedDuration::from_mins(70));
        restarts(count);
        assert_ok(&h.run(&check).await, "worker");
    }
}

#[tokio::test]
async fn new_unmanaged_container_reports_once() {
    let docker = Arc::new(FakeDocker::default());
    docker.set(vec![container("existing", false)]);
    let check = NewContainers::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    assert!(h.run(&check).await.observations.is_empty());

    let mut throwaway = container("scratch", false);
    throwaway.auto_remove = true;
    docker.set(vec![
        container("existing", false),
        container("adhoc", false),
        container("stack-app", true),
        throwaway,
    ]);
    h.advance(MINUTE);
    let report = h.run(&check).await;
    assert_eq!(targets(&report), ["adhoc"]);
    assert_firing(&report, "adhoc", Severity::Info);
    assert!(is_immediate(find(&report, "adhoc"), &check));

    h.advance(MINUTE);
    assert!(h.run(&check).await.observations.is_empty());
}

#[tokio::test]
async fn baseline_entry_expires_after_seven_days() {
    let docker = Arc::new(FakeDocker::default());
    docker.set(vec![container("plex", true)]);
    let check = ExpectedContainers::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.run(&check).await;
    h.advance(MINUTE);
    h.run(&check).await;

    docker.set(Vec::new());
    h.advance(SignedDuration::from_hours(24 * 6));
    assert_firing(&h.run(&check).await, "plex", Severity::Warning);

    h.advance(SignedDuration::from_hours(24) + MINUTE);
    let expired = h.run(&check).await;
    assert!(expired.observations.is_empty(), "{expired:#?}");
    let fact = expired
        .facts
        .iter()
        .find(|f| f.key == "baseline.expired")
        .unwrap();
    assert_eq!(fact.value, FactValue::Text("plex".to_owned()));
}

#[tokio::test]
async fn unhealthy_container_fires_and_healthy_resolves() {
    let docker = Arc::new(FakeDocker::default());
    let mut sick = container("api", true);
    sick.health = Some(Health::Unhealthy);
    let mut fine = container("db", true);
    fine.health = Some(Health::Healthy);
    docker.set(vec![sick, fine, container("nohealth", true)]);
    let check = UnhealthyContainers::new(deps(&docker, ContainerPolicy::default())).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    let report = h.run(&check).await;
    assert_eq!(firing_targets(&report), ["api"]);
    assert_ok(&report, "db");
    assert_eq!(report.observations.len(), 2);
}

#[tokio::test]
async fn ignored_containers_are_skipped_and_events_are_logged() {
    let docker = Arc::new(FakeDocker::default());
    docker.set(vec![container("watchtower", true), container("plex", true)]);
    let at = domovoi_testkit::build::T0 + SignedDuration::from_secs(30);
    docker.set_events(vec![ContainerEvent {
        at,
        name: "plex".to_owned(),
        action: "die".to_owned(),
        image: None,
    }]);
    let policy = ContainerPolicy {
        ignore: BTreeSet::from(["watchtower".to_owned()]),
        ..ContainerPolicy::default()
    };
    let check = ExpectedContainers::new(deps(&docker, policy)).unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.run(&check).await;
    h.advance(MINUTE);
    let report = h.run(&check).await;
    assert_eq!(targets(&report), ["plex"]);

    let log = EventLog::from_state(&h.state);
    assert_eq!(log.events.len(), 1);
    assert_eq!(log.events[0].action, "die");
}
