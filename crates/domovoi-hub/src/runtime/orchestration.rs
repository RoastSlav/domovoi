//! The operation service, started only while `[orchestration] enabled` is set.

use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{ActionCaps, Clock, TargetResolver};
use domovoi_engine::TriageHandle;
use domovoi_engine::events::DomainEvent;
use domovoi_engine::operations::{
    InvestigateDeps, InvestigateSettings, OperationDeps, OperationHandle, OperationService,
};
use domovoi_engine::targets::TargetService;
use domovoi_engine::{Evidence, PatternRedactor};
use domovoi_store::Store;
use tokio::sync::broadcast;
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
    /// The event bus an execution leaving approval is announced on.
    pub(super) bus: &'a broadcast::Sender<DomainEvent>,
    /// The triage service whose gates and cards investigations reuse; `None` when triage is
    /// off, where investigations do not start.
    pub(super) triage: Option<&'a TriageHandle>,
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
        bus,
        triage,
    } = input;
    if !config.orchestration.enabled {
        if config.triage.enabled {
            tracing::warn!(
                "triage is on but orchestration is off: incidents are not investigated automatically"
            );
        }
        return None;
    }
    let deps = OperationDeps {
        bus: bus.clone(),
        operations: store.clone(),
        experience: store.clone(),
        uow: store.clone(),
        effects: store.clone(),
        effect_read: store.clone(),
        advisories: store.clone(),
        incidents: store.clone(),
        hosts: store.clone(),
        evidence: evidence.clone(),
        registry: registry.clone(),
        // One resolver per runtime: the one triage already built, or a new one without it.
        resolver: triage.map_or_else(
            || -> Arc<dyn TargetResolver> {
                Arc::new(TargetService::new(
                    Arc::new(evidence.clone()),
                    store.clone(),
                ))
            },
            TriageHandle::resolver,
        ),
        observer: caps.observer,
        journal: caps.journal,
        handoff: caps.handoff,
        rerun: caps.rerun,
        redactor: redactor.clone(),
        clock: clock.clone(),
        shutdown: shutdown.clone(),
        investigate: triage.map(|triage| InvestigateDeps {
            triage: triage.clone(),
            // `POST /api/v1/operations/{id}/steps/{step}/answer` reaches the writer.
            settings: InvestigateSettings {
                allow_questions: true,
                ..InvestigateSettings::default()
            },
        }),
    };
    Some(OperationService::spawn(deps, settings::operations(config)))
}
