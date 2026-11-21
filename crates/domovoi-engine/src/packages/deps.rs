//! What the install service is built from: ports only. The agent link is reached through the
//! package capability object, never directly.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use domovoi_core::capability::CapabilityContract;
use domovoi_core::ports::{
    CapabilityRegistry, Clock, HostRepo, InstallRepo, PackageCaps, PackageFiles, TargetResolver,
    UnitOfWork,
};
use domovoi_core::{HostId, capability};
use jiff::SignedDuration;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

/// What the install service needs. Everything is a port; the engine does no I/O.
#[derive(Clone)]
pub struct PackageDeps {
    /// Stored artifacts, jobs, targets and host inventories, read only.
    pub installs: Arc<dyn InstallRepo>,
    /// One atomic commit for a state change, its journal row and its audit rows.
    pub uow: Arc<dyn UnitOfWork>,
    /// Registered hosts, read for their names.
    pub hosts: Arc<dyn HostRepo>,
    /// The canonical identity, enrolled key included, a plan binds.
    pub resolver: Arc<dyn TargetResolver>,
    /// Whether the hub offers a package contract on a host.
    pub registry: Arc<dyn CapabilityRegistry>,
    /// Availability, the session snapshot, the journal and the relay.
    pub caps: PackageCaps,
    /// The bytes of imported archives.
    pub files: Arc<dyn PackageFiles>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Stops the service.
    pub shutdown: CancellationToken,
}

/// The knobs of the install service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageSettings {
    /// How long a confirmation stays usable for starting the work.
    pub approval_window: SignedDuration,
    /// How many hosts relay files at the same time.
    pub max_relaying_hosts: usize,
    /// How often the worker looks again at targets that are waiting.
    pub wake_interval: SignedDuration,
    /// Extra time beyond a contract's timeout to wait for an agent's answer.
    pub result_grace: SignedDuration,
}

impl Default for PackageSettings {
    fn default() -> Self {
        Self {
            approval_window: SignedDuration::from_mins(10),
            max_relaying_hosts: 4,
            wake_interval: SignedDuration::from_secs(30),
            result_grace: SignedDuration::from_secs(30),
        }
    }
}

/// State shared by the handle, the worker and the reconciler.
pub(super) struct Shared {
    pub(super) deps: PackageDeps,
    pub(super) settings: PackageSettings,
    window_secs: AtomicI64,
    /// Rings when a job is created, a host authenticates or a retry is queued.
    pub(super) wake: Notify,
    /// Limits how many hosts relay files at once.
    pub(super) slots: Arc<Semaphore>,
    /// Hosts whose target this process is driving now. A dispatched target on such a host is
    /// never reconciled: its own run will settle it.
    busy: std::sync::Mutex<BTreeSet<HostId>>,
    contracts: Vec<CapabilityContract>,
}

impl Shared {
    pub(super) fn new(deps: PackageDeps, settings: PackageSettings) -> Self {
        Self {
            window_secs: AtomicI64::new(settings.approval_window.as_secs()),
            slots: Arc::new(Semaphore::new(settings.max_relaying_hosts.max(1))),
            deps,
            settings,
            wake: Notify::new(),
            busy: std::sync::Mutex::new(BTreeSet::new()),
            contracts: capability::package_contracts(),
        }
    }

    /// The current approval window.
    pub(super) fn approval_window(&self) -> SignedDuration {
        SignedDuration::from_secs(self.window_secs.load(Ordering::Relaxed))
    }

    /// Changes the approval window for jobs created from now on.
    pub(super) fn set_approval_window(&self, window: SignedDuration) {
        self.window_secs.store(window.as_secs(), Ordering::Relaxed);
    }

    /// The package contract with this id, as the hub's compiled table defines it.
    pub(super) fn contract(&self, id: &str) -> Option<&CapabilityContract> {
        self.contracts.iter().find(|c| c.id.as_str() == id)
    }

    /// Marks `host` as driven by this process. `false` when it already is.
    pub(super) fn begin(&self, host: HostId) -> bool {
        locked(&self.busy).insert(host)
    }

    /// Clears the mark of [`Shared::begin`].
    pub(super) fn end(&self, host: HostId) {
        locked(&self.busy).remove(&host);
    }
}

/// Locks `mutex`, recovering the data if a panicking task poisoned it.
fn locked<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
