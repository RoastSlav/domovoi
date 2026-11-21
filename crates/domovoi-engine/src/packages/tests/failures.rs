//! What each way of not finishing settles to: not run, failed or an unknown outcome.

use std::sync::Arc;

use domovoi_core::capability::Availability;
use domovoi_core::install::codes;
use domovoi_core::package::keys;
use domovoi_core::ports::{InstallRepo, LinkError, Presence};
use domovoi_core::{PackageOutputs, TargetStatus, TransferAnswer, TransferFrame};
use domovoi_testkit::link::PackageEvent;
use tokio::sync::Semaphore;

use super::world::{PAST_WINDOW, World};

const INSTALL: &str = "agent.package.install";

fn ends(world: &World) -> usize {
    world
        .link
        .transfer_frames()
        .iter()
        .filter(|frame| matches!(frame, TransferFrame::End { .. }))
        .count()
}

fn capabilities(world: &World) -> usize {
    world
        .link
        .package_events()
        .iter()
        .filter(|event| matches!(event, PackageEvent::Capability { .. }))
        .count()
}

#[tokio::test]
async fn expired_approval_is_not_run() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.handle.set_approval_window(jiff::SignedDuration::ZERO);
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::APPROVAL_EXPIRED);
    assert!(world.link.package_events().is_empty());
}

#[tokio::test]
async fn host_unknown_waits_then_runs_on_wake() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    world.set_availability(world.host, INSTALL, Availability::HostUnknown);
    let waiting = world.install("web-tools", "1.0.0").await;
    world.quiet().await;
    assert_eq!(world.target(&waiting).await.status, TargetStatus::Queued);
    assert!(world.link.package_events().is_empty());

    world.set_availability(world.host, INSTALL, Availability::Available);
    world.handle.wake();
    let done = world.settled(&waiting).await;
    assert_eq!(done.status, TargetStatus::Succeeded);

    // A host that never shows up lets the approval run out; the timer notices without a wake.
    world.set_availability(
        world.host,
        INSTALL,
        Availability::HostAbsent {
            since: domovoi_testkit::build::T0,
        },
    );
    let stale = world.install("web-tools", "1.0.0").await;
    world.quiet().await;
    assert_eq!(world.target(&stale).await.status, TargetStatus::Queued);
    world.clock.advance(PAST_WINDOW);
    let expired = world.settled(&stale).await;
    assert_eq!(expired.status, TargetStatus::NotRun);
    assert_eq!(expired.failure.unwrap().code(), codes::APPROVAL_EXPIRED);
}

#[tokio::test]
async fn not_advertised_settles_before_any_transfer() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.set_availability(world.host, INSTALL, Availability::NotInstalled);
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::NOT_ADVERTISED);
    assert!(world.link.package_events().is_empty());
    // It never reached the claim.
    assert_eq!(
        world.transitions(&target).await.last().unwrap(),
        &("queued".to_owned(), "not_run".to_owned())
    );

    world.set_availability(
        world.host,
        INSTALL,
        Availability::HostIncompatible {
            reported: "1.1".to_owned(),
        },
    );
    let again = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&again).await;
    assert_eq!(done.failure.unwrap().code(), codes::AGENT_INCOMPATIBLE);
}

#[tokio::test]
async fn transfer_failure_is_not_run_and_never_dispatches() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let gate = Arc::new(Semaphore::new(0));
    let entered = world.link.hold_transfer_ends(gate.clone());
    let target = world.install("web-tools", "1.0.0").await;
    entered.notified().await;
    let transfer = world.link.transfer_frames().last().unwrap().transfer();
    world.link.answer_transfer(
        transfer,
        Ok(TransferAnswer::Failed {
            code: "disk_full".to_owned(),
            detail: "no space left".to_owned(),
        }),
    );
    gate.add_permits(8);
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    let failure = done.failure.unwrap();
    assert_eq!(failure.code(), codes::TRANSFER_FAILED);
    assert!(failure.message().contains("no space left"));
    assert_eq!(capabilities(&world), 0);
    assert!(done.dispatched_session.is_none());
}

#[tokio::test]
async fn missing_transfer_result_is_a_failed_transfer() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let gate = Arc::new(Semaphore::new(0));
    let entered = world.link.hold_transfer_ends(gate.clone());
    let target = world.install("web-tools", "1.0.0").await;
    entered.notified().await;
    let transfer = world.link.transfer_frames().last().unwrap().transfer();
    world.link.answer_transfer(
        transfer,
        Err(LinkError::Indeterminate("no reply".to_owned())),
    );
    gate.add_permits(8);
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::TRANSFER_FAILED);
    assert_eq!(capabilities(&world), 0);
}

#[tokio::test]
async fn artifact_missing_or_corrupt_is_not_run_without_end_frame() {
    let world = World::new().await;
    let artifact = world
        .import("web-tools", "1.0.0", &vec![7_u8; 100_000])
        .await;

    world.files.remove(&artifact.archive_sha256);
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.failure.unwrap().code(), codes::ARTIFACT_MISSING);
    assert!(
        world.link.package_events().is_empty(),
        "no frame may be sent for a missing file"
    );

    // The same length with other bytes: the index and the file disagree.
    world
        .files
        .store_as(&artifact.archive_sha256, &vec![8_u8; 100_000]);
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::ARTIFACT_CORRUPT);
    assert_eq!(
        ends(&world),
        0,
        "End is never sent for bytes that do not match"
    );
    assert_eq!(capabilities(&world), 0);
    assert!(
        !world.link.transfer_frames().is_empty(),
        "the bytes were hashed while sending"
    );
}

#[tokio::test]
async fn agent_refusal_codes_pass_through() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let cases = [
        (
            "bad_signature: the signature does not verify",
            "bad_signature",
        ),
        (
            "untrusted_key: the key is not trusted here",
            "untrusted_key",
        ),
        ("rate_limited: too many requests", "rate_limited"),
        ("allow_risk_denied: not allowed", "allow_risk_denied"),
        ("authorization_expired: too late", "authorization_expired"),
        ("something the hub never heard of", "agent_refused"),
    ];
    for (message, code) in cases {
        world.agent_answers(LinkError::Rejected(message.to_owned()));
        let target = world.install("web-tools", "1.0.0").await;
        let done = world.settled(&target).await;
        assert_eq!(done.status, TargetStatus::Failed, "{message}");
        assert_eq!(done.failure.unwrap().code(), code, "{message}");
    }
}

#[tokio::test]
async fn handoff_failure_is_not_run() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    let link = world.link.clone();
    let host = world.host;
    world.link.on_start_capability(move |_, _| {
        link.set_presence(host, Presence::Disconnected { last_seen: None });
    });
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::HANDOFF_FAILED);
    assert_eq!(
        capabilities(&world),
        0,
        "a failed hand-off leaves nothing queued"
    );
}

#[tokio::test]
async fn session_change_between_transfer_and_dispatch_is_not_run() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    let link = world.link.clone();
    let host = world.host;
    world.link.on_start_capability(move |_, _| {
        link.new_session(host);
    });
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::SESSION_CHANGED);
    assert_eq!(capabilities(&world), 0);
}

#[tokio::test]
async fn result_mismatch_is_outcome_unknown() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    let mut outputs = World::install_outputs(&artifact);
    outputs.insert(keys::OUT_ARCHIVE_SHA256.to_owned(), "0".repeat(64));
    world
        .link
        .on_capability(move |_, _| Ok(PackageOutputs(outputs.clone())));
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::OutcomeUnknown);
    assert_eq!(done.failure.unwrap().code(), codes::RESULT_MISMATCH);
    assert_eq!(world.audit("package.install_mismatch").await.len(), 1);
    assert_eq!(world.audit("package.outcome_unknown").await.len(), 1);
    assert!(
        InstallRepo::inventory(&*world.store, world.host)
            .await
            .unwrap()
            .is_empty()
    );
}
