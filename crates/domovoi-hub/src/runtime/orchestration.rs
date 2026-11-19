//! The operation service, started only while `[orchestration] enabled` is set, and the
//! reconciler that tells it, beside the action service, when an agent authenticated.

use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::HostId;
use domovoi_core::ports::{ActionCaps, Clock, HostReconciler};
use domovoi_engine::operations::{OperationDeps, OperationHandle, OperationService};
use domovoi_engine::targets::TargetService;
use domovoi_engine::{Evidence, PatternRedactor};
use domovoi_store::Store;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::settings;
use crate::capability::HubRegistry;

/// What the operation service is built from: values the runtime already holds.
pub(super) struct OperationBuild<'a> {
    pub(super) config: &'a HubConfig,
    pub(super) store: &'a Arc<Store>,
    pub(super) evidence: &'a Evidence,
    pub(super) registry: &'a Arc<HubRegistry>,
    pub(super) caps: ActionCaps,
    pub(super) redactor: &'a Arc<PatternRedactor>,
    pub(super) clock: &'a Arc<dyn Clock>,
    pub(super) shutdown: &'a CancellationToken,
}

/// Starts the operation service when the flag is on; with it off nothing is built or
/// spawned.
pub(super) fn build(input: OperationBuild<'_>) -> Option<(OperationHandle, JoinHandle<()>)> {
    let OperationBuild {
        config,
        store,
        evidence,
        registry,
        caps,
        redactor,
        clock,
        shutdown,
    } = input;
    if !config.orchestration.enabled {
        return None;
    }
    let deps = OperationDeps {
        operations: store.clone(),
        uow: store.clone(),
        effects: store.clone(),
        effect_read: store.clone(),
        advisories: store.clone(),
        incidents: store.clone(),
        hosts: store.clone(),
        evidence: evidence.clone(),
        registry: registry.clone(),
        resolver: Arc::new(TargetService::new(
            Arc::new(evidence.clone()),
            store.clone(),
        )),
        observer: caps.observer,
        journal: caps.journal,
        handoff: caps.handoff,
        rerun: caps.rerun,
        redactor: redactor.clone(),
        clock: clock.clone(),
        shutdown: shutdown.clone(),
    };
    Some(OperationService::spawn(deps, settings::operations(config)))
}

/// Fans one authenticated `hello` out to every service that reconciles dispatched work.
pub(crate) struct CompositeReconciler {
    members: Vec<Arc<dyn HostReconciler>>,
}

impl CompositeReconciler {
    /// A reconciler that tells every one of `members`, concurrently.
    #[must_use]
    pub(crate) fn new(members: Vec<Arc<dyn HostReconciler>>) -> Self {
        Self { members }
    }
}

#[async_trait::async_trait]
impl HostReconciler for CompositeReconciler {
    async fn reconcile_host(&self, host: HostId) {
        futures::future::join_all(
            self.members
                .iter()
                .map(|member| member.reconcile_host(host)),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_core::HostId;
    use domovoi_core::ports::HostReconciler;
    use domovoi_testkit::RecordingReconciler;

    use super::CompositeReconciler;

    #[tokio::test]
    async fn the_composite_reconciles_through_every_member() {
        let first = Arc::new(RecordingReconciler::new());
        let second = Arc::new(RecordingReconciler::new());
        let composite = CompositeReconciler::new(vec![first.clone(), second.clone()]);
        let host = HostId::new();

        composite.reconcile_host(host).await;

        assert_eq!(first.hosts(), vec![host]);
        assert_eq!(second.hosts(), vec![host]);
    }
}
