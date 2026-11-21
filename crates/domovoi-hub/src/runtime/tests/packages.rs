//! The runtime's package wiring: the reconciler that serves both services, and the relay
//! capability over the one link.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::{AgentLink, HostReconciler, Presence};
use domovoi_core::{
    ExecutionId, HostId, JournalEpoch, PackageCall, PackageDispatch, PackageOutputs, PlanHash,
    TransferAnswer, TransferFrame, TransferKind,
};
use domovoi_testkit::FakeAgentLink;
use domovoi_testkit::link::PackageEvent;
use jiff::Timestamp;
use uuid::Uuid;

use crate::runtime::caps::package_caps;
use crate::runtime::packages::CompositeReconciler;

struct Recording {
    name: &'static str,
    log: Arc<Mutex<Vec<(&'static str, HostId)>>>,
}

#[async_trait]
impl HostReconciler for Recording {
    async fn reconcile_host(&self, host: HostId) {
        self.log
            .lock()
            .map(|mut log| log.push((self.name, host)))
            .ok();
    }
}

#[tokio::test]
async fn composite_reconciler_calls_actions_then_packages() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let composite = CompositeReconciler::new(vec![
        Arc::new(Recording {
            name: "actions",
            log: log.clone(),
        }),
        Arc::new(Recording {
            name: "packages",
            log: log.clone(),
        }),
    ]);
    let host = HostId::new();
    composite.reconcile_host(host).await;
    let other = HostId::new();
    composite.reconcile_host(other).await;
    let seen = log.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            ("actions", host),
            ("packages", host),
            ("actions", other),
            ("packages", other),
        ],
        "every reconciler runs for every host, in the order they were given"
    );
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[tokio::test]
async fn package_relay_adapter_forwards_the_three_operations() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    link.set_presence(
        host,
        Presence::Connected {
            since: at("2026-10-31T12:00:00Z"),
            version: "0.1.0".to_owned(),
        },
    );
    let epoch = JournalEpoch::from_uuid(Uuid::from_u128(7));
    link.set_journal_epoch(host, epoch);
    let session = link.session_id(host);
    let as_link: Arc<dyn AgentLink> = link.clone();
    let relay = package_caps(as_link).relay;
    let deadline = at("2026-10-31T12:05:00Z");

    let transfer = Uuid::now_v7();
    let begin = TransferFrame::Begin {
        transfer,
        kind: TransferKind::PackageArtifact,
        size: 3,
        sha256: [9; 32],
    };
    assert_eq!(
        relay
            .relay_transfer(host, begin.clone(), session, deadline)
            .await
            .unwrap(),
        None,
        "only the end of a transfer is answered"
    );
    let end = TransferFrame::End { transfer };
    assert_eq!(
        relay
            .relay_transfer(host, end.clone(), session, deadline)
            .await
            .unwrap(),
        Some(TransferAnswer::Stored)
    );

    let request = PackageDispatch {
        execution: ExecutionId::new(),
        call: PackageCall::Uninstall {
            package: PackageName::parse("web-tools").unwrap(),
            version: PackageVersion::parse("1.0.0").unwrap(),
        },
        plan_hash: "ab".repeat(32).parse::<PlanHash>().unwrap(),
        expires_at: at("2026-10-31T12:10:00Z"),
    };
    link.on_capability(|_, _| Ok(PackageOutputs(Default::default())));
    let handoff = relay
        .dispatch_package(host, request.clone(), session, epoch, deadline)
        .await
        .unwrap();
    let outputs = relay.package_result(host, handoff, deadline).await.unwrap();
    assert!(outputs.0.is_empty());

    let events = link.package_events();
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(matches!(
        &events[0],
        PackageEvent::Transfer { frame, session: s, .. } if *frame == begin && *s == session
    ));
    assert!(matches!(
        &events[1],
        PackageEvent::Transfer { frame, .. } if *frame == end
    ));
    assert!(matches!(
        &events[2],
        PackageEvent::Capability { dispatch, epoch: e, .. }
            if *dispatch == request && *e == epoch
    ));
    assert!(matches!(
        &events[3],
        PackageEvent::CapabilityResult { execution, .. } if *execution == request.execution
    ));
}
