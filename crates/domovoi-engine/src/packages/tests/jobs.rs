//! Creating, retrying and cancelling jobs, and what a job's targets do side by side.

use domovoi_core::capability::Availability;
use domovoi_core::ports::{InstallJobFilter, InstallRepo, LinkError, Page};
use domovoi_core::{InstallKind, PackageOutputs, TargetStatus, UserId};
use domovoi_testkit::approvals;

use super::world::World;
use crate::packages::{JobRequest, PackageError};

const INSTALL: &str = "agent.package.install";

async fn job_count(world: &World) -> usize {
    InstallRepo::jobs(
        &*world.store,
        &InstallJobFilter::default(),
        &Page::first(50),
    )
    .await
    .unwrap()
    .items
    .len()
}

#[tokio::test]
async fn one_active_target_per_host() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.set_availability(world.host, INSTALL, Availability::HostUnknown);
    let first = world.install("web-tools", "1.0.0").await;
    let request = world.request(InstallKind::Install, "web-tools", "1.0.0", &[world.host]);
    let err = world.create(request).await.unwrap_err();
    assert_eq!(err, PackageError::HostBusy(world.host));
    assert_eq!(job_count(&world).await, 1, "a refused job creates nothing");
    assert_eq!(world.target(&first).await.status, TargetStatus::Queued);
}

#[tokio::test]
async fn plan_hash_mismatch_refuses_create() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let request = world.request(InstallKind::Install, "web-tools", "1.0.0", &[world.host]);
    let plan = world.handle.plan(&request).await.unwrap();
    assert_eq!(plan.hosts.len(), 1);
    assert_eq!(plan.signer_label.as_deref(), Some("Owner"));
    // Another version is another plan.
    world.import("web-tools", "1.1.0", b"other archive").await;
    let other = world.request(InstallKind::Install, "web-tools", "1.1.0", &[world.host]);
    let stale = world.handle.plan(&other).await.unwrap().job_plan_hash;
    assert_ne!(stale, plan.job_plan_hash);
    let err = world
        .handle
        .create_job(
            JobRequest {
                plan: request,
                job_plan_hash: stale,
            },
            world.approval(),
            approvals::provenance(),
        )
        .await
        .unwrap_err();
    assert_eq!(err, PackageError::PlanChanged);
    assert_eq!(job_count(&world).await, 0);
    world.quiet().await;
    assert!(world.link.package_events().is_empty());
}

#[tokio::test]
async fn retry_creates_a_new_attempt_with_new_execution_and_authorization() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_answers(LinkError::Rejected("rate_limited: slow down".to_owned()));
    let first = world.install("web-tools", "1.0.0").await;
    let failed = world.settled(&first).await;
    assert_eq!(failed.status, TargetStatus::Failed);

    world.agent_installs(&artifact);
    let second_user = UserId::new();
    let again = world
        .handle
        .retry_target(
            failed.job,
            failed.host,
            failed.plan_hash,
            approvals::web_typed(second_user),
            approvals::provenance(),
        )
        .await
        .unwrap();
    assert_eq!(again.attempt, 2);
    assert_ne!(again.execution, failed.execution);
    assert_ne!(again.id, failed.id);
    assert_eq!(again.authorization.approval().user(), second_user);
    assert_ne!(failed.authorization.approval().user(), second_user);
    assert_eq!(
        again.plan_hash, failed.plan_hash,
        "the plan is the same; the execution is not"
    );
    let done = world.settled(&again).await;
    assert_eq!(done.status, TargetStatus::Succeeded);
    assert_eq!(
        world.target(&first).await.status,
        TargetStatus::Failed,
        "the old attempt is kept"
    );

    // Only the latest attempt, and only after it did not succeed.
    let err = world
        .handle
        .retry_target(
            failed.job,
            failed.host,
            failed.plan_hash,
            world.approval(),
            approvals::provenance(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, PackageError::Conflict(_)), "{err:?}");
}

#[tokio::test]
async fn retry_with_a_stale_plan_hash_is_refused() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_answers(LinkError::Rejected("rate_limited: slow down".to_owned()));
    let first = world.install("web-tools", "1.0.0").await;
    let failed = world.settled(&first).await;
    let wrong = world
        .handle
        .plan(&world.request(InstallKind::Uninstall, "web-tools", "1.0.0", &[world.host]))
        .await
        .unwrap()
        .hosts[0]
        .plan_hash;
    let err = world
        .handle
        .retry_target(
            failed.job,
            failed.host,
            wrong,
            world.approval(),
            approvals::provenance(),
        )
        .await
        .unwrap_err();
    assert_eq!(err, PackageError::PlanChanged);
}

#[tokio::test]
async fn partial_install_reports_per_host() {
    let world = World::new().await;
    let other = world.add_host("db").await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    let outputs = World::install_outputs(&artifact);
    world.link.on_capability(move |host, _| {
        if host == other {
            Err(LinkError::Rejected(
                "untrusted_key: not trusted here".to_owned(),
            ))
        } else {
            Ok(PackageOutputs(outputs.clone()))
        }
    });
    let request = world.request(
        InstallKind::Install,
        "web-tools",
        "1.0.0",
        &[world.host, other],
    );
    let (_, targets) = world.create(request).await.unwrap();
    assert_eq!(targets.len(), 2);
    for target in &targets {
        let done = world.settled(target).await;
        if done.host == other {
            assert_eq!(done.status, TargetStatus::Failed);
            assert_eq!(done.failure.unwrap().code(), "untrusted_key");
        } else {
            assert_eq!(done.status, TargetStatus::Succeeded);
        }
    }
    let kept = InstallRepo::inventory(&*world.store, world.host)
        .await
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert!(
        InstallRepo::inventory(&*world.store, other)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn cancel_only_queued_targets() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    world.set_availability(world.host, INSTALL, Availability::HostUnknown);
    let waiting = world.install("web-tools", "1.0.0").await;
    let cancelled = world
        .handle
        .cancel_target(waiting.job, waiting.host, world.user)
        .await
        .unwrap();
    assert_eq!(cancelled.status, TargetStatus::Cancelled);
    assert_eq!(world.audit("package.cancelled").await.len(), 1);
    // The host showing up afterwards runs nothing.
    world.set_availability(world.host, INSTALL, Availability::Available);
    world.handle.wake();
    world.quiet().await;
    assert!(world.link.package_events().is_empty());
    let err = world
        .handle
        .cancel_target(waiting.job, waiting.host, world.user)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, PackageError::Conflict(text) if text.contains("already started")),
        "{err:?}"
    );

    let started = world.install("web-tools", "1.0.0").await;
    world.settled(&started).await;
    let err = world
        .handle
        .cancel_target(started.job, started.host, world.user)
        .await
        .unwrap_err();
    assert!(matches!(err, PackageError::Conflict(_)));
}
