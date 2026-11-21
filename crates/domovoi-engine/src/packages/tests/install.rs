//! The happy path and what it must never do.

use std::sync::{Arc, Mutex};

use domovoi_core::install::codes;
use domovoi_core::package::keys;
use domovoi_core::{PackageOutputs, TargetStatus, TransferFrame, TransferKind};
use domovoi_testkit::link::PackageEvent;

use super::world::World;
use crate::packages::PackageExecError;
use crate::packages::execute::AuthorizedPackageTarget;

/// An archive of three full chunks and a short one.
fn archive() -> Vec<u8> {
    (0..200_000_u32)
        .map(|n| u8::try_from(n % 251).unwrap())
        .collect()
}

#[tokio::test]
async fn install_runs_transfers_then_capability_in_order() {
    let world = World::new().await;
    let bytes = archive();
    let artifact = world.import("web-tools", "1.0.0", &bytes).await;
    world.agent_installs(&artifact);
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::Succeeded, "{:?}", done.failure);
    assert!(done.outputs_confirmed);

    let events = world.link.package_events();
    let frames: Vec<&PackageEvent> = events.iter().collect();
    // Begin A, 4 chunks, End A, Begin B, 1 chunk, End B, the request, then the wait.
    assert_eq!(frames.len(), 1 + 4 + 1 + 1 + 1 + 1 + 1 + 1);
    let PackageEvent::Transfer {
        frame:
            TransferFrame::Begin {
                transfer: a,
                kind,
                size,
                sha256,
            },
        ..
    } = frames[0]
    else {
        panic!(
            "the first event is not the archive's Begin: {:?}",
            frames[0]
        );
    };
    assert_eq!(*kind, TransferKind::PackageArtifact);
    assert_eq!(*size, 200_000);
    assert_eq!(sha256, World::digest_of(&bytes).as_bytes());
    for (seq, event) in frames[1..5].iter().enumerate() {
        let PackageEvent::Transfer {
            frame:
                TransferFrame::Chunk {
                    transfer,
                    seq: got,
                    bytes: chunk,
                },
            ..
        } = event
        else {
            panic!("not a chunk: {event:?}");
        };
        assert_eq!(transfer, a);
        assert_eq!(*got, u64::try_from(seq).unwrap());
        assert_eq!(chunk.len(), if seq < 3 { 65_536 } else { 3_392 });
    }
    assert!(matches!(
        frames[5],
        PackageEvent::Transfer { frame: TransferFrame::End { transfer }, .. } if transfer == a
    ));
    let PackageEvent::Transfer {
        frame: TransferFrame::Begin {
            transfer: b, kind, ..
        },
        ..
    } = frames[6]
    else {
        panic!("the signature does not follow: {:?}", frames[6]);
    };
    assert_eq!(*kind, TransferKind::PackageManifest);
    assert!(matches!(
        frames[7],
        PackageEvent::Transfer {
            frame: TransferFrame::Chunk { .. },
            ..
        }
    ));
    assert!(matches!(
        frames[8],
        PackageEvent::Transfer { frame: TransferFrame::End { transfer }, .. } if transfer == b
    ));
    let PackageEvent::Capability { dispatch, .. } = frames[9] else {
        panic!("the request does not follow the transfers: {:?}", frames[9]);
    };
    assert_eq!(dispatch.execution, done.execution);
    assert_eq!(dispatch.plan_hash, done.plan_hash);
    assert!(matches!(frames[10], PackageEvent::CapabilityResult { .. }));
    assert_eq!(done.artifact_transfer, Some(*a));
    assert_eq!(done.signature_transfer, Some(*b));
    assert_eq!(
        world.transitions(&target).await,
        [
            ("new", "queued"),
            ("queued", "transferring"),
            ("transferring", "dispatched"),
            ("dispatched", "succeeded"),
        ]
        .map(|(from, to)| (from.to_owned(), to.to_owned())),
    );
    for kind in [
        "package.install_requested",
        "package.relayed",
        "package.install_dispatched",
        "package.installed",
    ] {
        assert_eq!(world.audit(kind).await.len(), 1, "{kind}");
    }
}

#[tokio::test]
async fn dispatch_is_committed_before_the_request_is_sent() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let store = world.store.clone();
    let record = seen.clone();
    world.link.on_start_capability(move |_, request| {
        let target = ready(domovoi_core::ports::InstallRepo::non_terminal_targets(
            &*store,
        ))
        .unwrap()
        .into_iter()
        .find(|t| t.execution == request.execution);
        record.lock().unwrap().push(target);
    });
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::Succeeded);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let at_send = seen[0]
        .as_ref()
        .expect("the target was committed before the send");
    assert_eq!(at_send.status, TargetStatus::Dispatched);
    assert!(at_send.dispatched_session.is_some());
    assert!(at_send.dispatched_epoch.is_some());
    assert!(at_send.artifact_transfer.is_some() && at_send.signature_transfer.is_some());
    assert!(at_send.started_at.is_some());
}

/// Polls a future that needs no I/O once; the in-memory store answers at once.
fn ready<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("the in-memory store was not ready"),
    }
}

#[tokio::test]
async fn no_capability_without_human_authorization() {
    let world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.set_availability(
        world.host,
        "agent.package.install",
        domovoi_core::capability::Availability::HostUnknown,
    );
    let target = world.install("web-tools", "1.0.0").await;
    let err = AuthorizedPackageTarget::of(target, None).unwrap_err();
    assert_eq!(err, PackageExecError::NotAuthorized);
    assert_eq!(err.not_run().unwrap().0, "not_authorized");
    world.quiet().await;
    assert!(
        world.link.package_events().is_empty(),
        "nothing may reach the agent"
    );
}

#[tokio::test]
async fn install_never_enables() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    // An agent that reports the package enabled is not what was asked for.
    let mut outputs = World::install_outputs(&artifact);
    outputs.insert(keys::OUT_ENABLED.to_owned(), "true".to_owned());
    world
        .link
        .on_capability(move |_, _| Ok(PackageOutputs(outputs.clone())));
    let target = world.install("web-tools", "1.0.0").await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::OutcomeUnknown);
    assert_eq!(done.failure.unwrap().code(), codes::RESULT_MISMATCH);
    // The request carries no argument that could enable it.
    let request = world
        .link
        .package_events()
        .into_iter()
        .find_map(|event| match event {
            PackageEvent::Capability { dispatch, .. } => Some(dispatch),
            _ => None,
        })
        .unwrap();
    let args = request.call.args();
    assert_eq!(
        args.keys().copied().collect::<Vec<_>>(),
        [
            keys::ARG_ARTIFACT_TRANSFER,
            keys::ARG_PACKAGE,
            keys::ARG_SIGNATURE_TRANSFER,
            keys::ARG_VERSION
        ]
    );
}
