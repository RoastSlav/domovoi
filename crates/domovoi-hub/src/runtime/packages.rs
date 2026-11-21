//! The package store and the install service, built once at start-up.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_config::hub::HubConfig;
use domovoi_core::HostId;
use domovoi_core::ports::{Clock, HostReconciler, PackageCaps, TargetResolver};
use domovoi_engine::operations::OperationHandle;
use domovoi_engine::packages::{InstallService, PackageDeps, PackageHandle, PackageSettings};
use domovoi_engine::targets::TargetService;
use domovoi_engine::{Evidence, TriageHandle};
use domovoi_store::Store;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::RuntimeError;
use crate::capability::HubRegistry;
use crate::packages::{PackageImporter, PackageKeys, PackageStore};

/// What the package services are built from: values the runtime already holds.
pub(super) struct PackageBuild<'a> {
    pub(super) config: &'a HubConfig,
    pub(super) store: &'a Arc<Store>,
    pub(super) caps: PackageCaps,
    pub(super) registry: &'a Arc<HubRegistry>,
    pub(super) evidence: &'a Evidence,
    pub(super) triage: Option<&'a TriageHandle>,
    pub(super) clock: &'a Arc<dyn Clock>,
    pub(super) shutdown: &'a CancellationToken,
}

/// The install service's handle and task, and the importer the web layer drives.
pub(super) struct Packages {
    pub(super) handle: PackageHandle,
    pub(super) importer: Arc<PackageImporter>,
    pub(super) task: JoinHandle<()>,
}

/// Opens the package store, clears what an earlier run left in it, and starts the install
/// service. The service runs whether or not `[packages] enabled` is set, so a target that was
/// dispatched before packages were switched off is still reconciled.
pub(super) async fn build(input: PackageBuild<'_>) -> Result<Packages, RuntimeError> {
    let PackageBuild {
        config,
        store,
        caps,
        registry,
        evidence,
        triage,
        clock,
        shutdown,
    } = input;
    let keys = Arc::new(PackageKeys::from_config(&config.packages)?);
    let files = PackageStore::open(&config.hub.data_dir)
        .await
        .map_err(|err| {
            RuntimeError::Packages(format!("could not open the package store: {err}"))
        })?;
    let importer = Arc::new(PackageImporter::new(
        config.packages.clone(),
        keys,
        files.clone(),
        store.clone(),
        store.clone(),
        clock.clone(),
    ));
    match importer.sweep().await {
        Ok(report) if report.tmp_files.is_empty() && report.unindexed.is_empty() => {}
        Ok(report) => tracing::info!(
            partial_uploads = report.tmp_files.len(),
            unindexed = report.unindexed.len(),
            "removed leftover package files"
        ),
        Err(err) => tracing::warn!(%err, "could not clean the package store"),
    }
    let resolver = triage.map_or_else(
        || -> Arc<dyn TargetResolver> {
            Arc::new(TargetService::new(
                Arc::new(evidence.clone()),
                store.clone(),
            ))
        },
        TriageHandle::resolver,
    );
    let (handle, task) = InstallService::spawn(
        PackageDeps {
            installs: store.clone(),
            uow: store.clone(),
            hosts: store.clone(),
            resolver,
            registry: registry.clone(),
            caps,
            files: Arc::new(files),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
        },
        PackageSettings {
            approval_window: config.packages.approval_window.as_signed(),
            ..PackageSettings::default()
        },
    );
    Ok(Packages {
        handle,
        importer,
        task,
    })
}

/// The reconciler the agent server calls when a host authenticates: the operation service
/// when it runs, then the install service. Set once, because the server keeps only the first.
pub(super) fn reconciler(
    operations: Option<&OperationHandle>,
    packages: &PackageHandle,
) -> Arc<CompositeReconciler> {
    let mut parts: Vec<Arc<dyn HostReconciler>> = Vec::new();
    if let Some(operations) = operations {
        parts.push(Arc::new(operations.clone()));
    }
    parts.push(Arc::new(packages.clone()));
    Arc::new(CompositeReconciler::new(parts))
}

/// Tells every reconciler about a host that authenticated, one after the other and in the
/// order given: the action and operation service first, then the install service. Each one
/// logs its own failures; none is skipped because another ran first.
pub(crate) struct CompositeReconciler {
    parts: Vec<Arc<dyn HostReconciler>>,
}

impl CompositeReconciler {
    /// Reconciles with each of `parts` in turn.
    pub(crate) fn new(parts: Vec<Arc<dyn HostReconciler>>) -> Self {
        Self { parts }
    }
}

#[async_trait]
impl HostReconciler for CompositeReconciler {
    async fn reconcile_host(&self, host: HostId) {
        for part in &self.parts {
            part.reconcile_host(host).await;
        }
    }
}
