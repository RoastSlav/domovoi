//! Building the triage service, and what it needs from the runtime that assembled it.

use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{Clock, ProcessExecutor};
use domovoi_engine::triage::{TriageDeps, TriageHandle, TriageService};
use domovoi_engine::{DomainEvent, Evidence, HostCatalog, ManagerHandle, PatternRedactor};
use domovoi_store::Store;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{AdapterOverrides, RuntimeError, settings};

/// Everything the triage model chain is built from, borrowed from the runtime.
pub(super) struct TriageBuild<'a> {
    pub(super) config: &'a HubConfig,
    pub(super) store: &'a Arc<Store>,
    pub(super) evidence: Evidence,
    pub(super) catalog: HostCatalog,
    pub(super) capabilities: Arc<dyn domovoi_core::ports::CapabilityRegistry>,
    pub(super) manager: &'a ManagerHandle,
    pub(super) bus: &'a broadcast::Sender<DomainEvent>,
    pub(super) clock: &'a Arc<dyn Clock>,
    pub(super) redactor: &'a Arc<PatternRedactor>,
    pub(super) executor: Option<Arc<dyn ProcessExecutor>>,
    pub(super) shutdown: &'a CancellationToken,
    pub(super) overrides: &'a AdapterOverrides,
    pub(super) actions: domovoi_engine::ActionHandle,
}

/// Starts triage when it is enabled, with the primary provider and every fallback the
/// configuration and the adapters name.
pub(super) async fn build(
    input: TriageBuild<'_>,
) -> Result<(Option<TriageHandle>, Option<JoinHandle<()>>), RuntimeError> {
    let TriageBuild {
        config,
        store,
        evidence,
        catalog,
        capabilities,
        manager,
        bus,
        clock,
        redactor,
        executor,
        shutdown,
        overrides,
        actions,
    } = input;
    if !config.triage.enabled {
        return Ok((None, None));
    }
    let (primary, configured_fallbacks) = match (&overrides.primary, &config.llm.primary) {
        (Some(provider), Some(primary_config)) => (
            provider.clone(),
            settings::fallback_models_for(primary_config, executor.clone(), clock)?,
        ),
        (Some(provider), None) => (provider.clone(), Vec::new()),
        (None, Some(primary_config)) => {
            let chain = settings::chain_for(primary_config, executor.clone(), clock)?;
            let head: Arc<dyn domovoi_core::ports::LlmProvider> =
                chain.iter().map(Arc::clone).next().ok_or_else(|| {
                    RuntimeError::Chat("triage is enabled without a primary provider".to_owned())
                })?;
            (head, chain.iter().skip(1).map(Arc::clone).collect())
        }
        (None, None) => {
            return Err(RuntimeError::Chat(
                "triage is enabled without a primary provider".to_owned(),
            ));
        }
    };
    let tail = match (&overrides.secondary, &config.llm.secondary) {
        (Some(provider), None) => vec![provider.clone()],
        (Some(provider), Some(secondary)) => std::iter::chain(
            std::iter::once(provider.clone()),
            settings::fallback_models_for(secondary, executor.clone(), clock)?
                .iter()
                .map(Arc::clone),
        )
        .collect(),
        (None, Some(secondary)) => settings::chain_for(secondary, executor.clone(), clock)?,
        (None, None) => Vec::new(),
    };
    let fallbacks = std::iter::chain(configured_fallbacks, tail).collect();
    let (handle, task) = TriageService::spawn(
        TriageDeps {
            primary,
            fallbacks,
            evidence,
            catalog,
            capabilities,
            incidents: store.clone(),
            signals: store.clone(),
            notes: store.clone(),
            hosts: store.clone(),
            actions: store.clone(),
            actions_service: actions,
            cache: store.clone(),
            usage: store.clone(),
            admission: store.clone(),
            advisories: store.clone(),
            redactor: redactor.clone(),
            clock: clock.clone(),
            manager: manager.clone(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        settings::triage(config),
    );
    Ok((Some(handle), Some(task)))
}
