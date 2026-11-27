//! Verification, installation and removal of signed capability packages.
//!
//! The agent decides alone whether a package installs: it checks the detached signature
//! against keys the owner put in `agent.toml`, audits the archive without writing anything,
//! and only then extracts. Nothing here enables or runs a package's skills.

pub mod archive;
pub mod enable;
pub mod entry;
pub mod error;
pub mod events;
pub mod extract;
pub mod keys;
pub mod layout;
pub mod outcome;
pub mod registry;
pub mod staging;
pub mod transfer;
pub mod verify;

mod install;
mod list;
mod recover;
mod uninstall;

pub use error::PackageError;
pub use events::{EventKind, EventSink, PackageEvent, TracingEvents};
pub use keys::KeySet;
pub use layout::StatvfsSpace;
pub use outcome::{
    EnableOutcome, EnableRequest, ExecutionDenied, InstallOutcome, InstallRequest, ListOutcome,
    ResolvedPackage, UninstallOutcome, UninstallRequest,
};
pub use staging::{ClaimedTransfer, Staging};
pub use transfer::TransferReceiver;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domovoi_config::{PackageLimits, PackagesSection};
use domovoi_core::package::{PackageFailureCode, PrivilegeBoundary};
use domovoi_core::ports::Clock;

/// Which package capabilities this agent offers, from `[packages]` in `agent.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackageSurface {
    /// `agent.package.list` is offered (the `[packages]` section exists).
    pub list: bool,
    /// `agent.package.install` and `agent.package.uninstall` are offered
    /// (`allow_install = true`).
    pub mutate: bool,
}

/// Free disk space, behind a port so tests can fake it.
pub trait SpaceProbe: Send + Sync {
    /// Bytes available to the agent on the filesystem holding `path`.
    ///
    /// # Errors
    /// The I/O error when the filesystem cannot be queried.
    fn free_bytes(&self, path: &Path) -> std::io::Result<u64>;
}

/// The directories a package service works with.
#[derive(Debug, Clone)]
pub struct PackageDirs {
    /// Where packages are installed.
    pub packages_dir: PathBuf,
    /// The agent's state directory; `packages_dir` may not overlap it.
    pub state_dir: PathBuf,
    /// The owner's skills directory; `packages_dir` may not overlap it, and its skill names
    /// are reserved.
    pub skills_dir: PathBuf,
    /// Further roots `packages_dir` may not overlap (the system and helper paths); tests
    /// replace the default list with temporary paths.
    pub protected: Vec<PathBuf>,
}

impl PackageDirs {
    /// The directories with the default protected roots.
    #[must_use]
    pub fn new(packages_dir: PathBuf, state_dir: PathBuf, skills_dir: PathBuf) -> Self {
        Self {
            packages_dir,
            state_dir,
            skills_dir,
            protected: layout::default_protected_roots(),
        }
    }

    /// Every root `packages_dir` must stay clear of.
    #[must_use]
    pub fn protected_roots(&self) -> Vec<PathBuf> {
        let mut roots = self.protected.clone();
        roots.push(self.state_dir.clone());
        roots.push(self.skills_dir.clone());
        roots
    }
}

/// What the service needs from the outside world.
#[derive(Clone)]
pub struct PackageDeps {
    /// The time source.
    pub clock: Arc<dyn Clock>,
    /// Whether the agent runs as root.
    pub runs_as_root: bool,
    /// Whether the privilege boundary is in effect: the agent is the `domovoi-agent` account
    /// with exactly the expected groups. Decided once at agent start by the binary.
    pub boundary_in_effect: bool,
    /// `<os>-<arch>`, matched against a package's `platforms`.
    pub platform: String,
    /// Where audit events go.
    pub events: Arc<dyn EventSink>,
    /// Free-space probe.
    pub space: Arc<dyn SpaceProbe>,
}

impl PackageDeps {
    /// Production wiring: tracing events, `statvfs`, and this build's platform.
    #[must_use]
    pub fn production(clock: Arc<dyn Clock>, runs_as_root: bool, boundary_in_effect: bool) -> Self {
        Self {
            clock,
            runs_as_root,
            boundary_in_effect,
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            events: Arc::new(TracingEvents),
            space: Arc::new(StatvfsSpace),
        }
    }
}

/// The steps of an install (and uninstall) where a test can stop the process without
/// cleanup, to rehearse a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStep {
    /// After the audit pass, before anything is written.
    AfterAudit,
    /// After extraction into the staging tree.
    AfterExtract,
    /// After the tree was renamed into place, before the registry write.
    AfterRename,
    /// After the registry write.
    AfterRegistry,
    /// After the replaced version's files were removed.
    AfterOldRemoved,
    /// Uninstall: before the registry write.
    BeforeUninstallRegistry,
    /// Uninstall: after the registry write, before the files are removed.
    AfterUninstallRegistry,
}

/// Test-only fault switches.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Faults {
    /// Stop with an injected crash when this step is reached.
    pub crash_after: Option<InstallStep>,
    /// Failures for the registry write.
    pub durable: crate::fs_atomic::DurableFaults,
}

/// The package service: installs, removes and lists packages, and resolves them for
/// execution. Built once at start; its key set never changes afterwards.
pub struct PackageService {
    dirs: PackageDirs,
    limits: PackageLimits,
    allow_install: bool,
    keys: KeySet,
    deps: PackageDeps,
    staging: Option<Arc<Staging>>,
    fence: Mutex<Option<PackageError>>,
    ops: Mutex<()>,
    /// Fires when the set of runnable package skills changed, so the link announces again.
    skills_changed: Arc<tokio::sync::Notify>,
    uid: u32,
    #[cfg(test)]
    pub(crate) faults: Mutex<Faults>,
}

impl std::fmt::Debug for PackageService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageService")
            .field("packages_dir", &self.dirs.packages_dir)
            .finish_non_exhaustive()
    }
}

impl PackageService {
    /// Opens `packages_dir`, runs recovery and builds the key set.
    ///
    /// An unusable directory or a corrupt registry yields a *fenced* service that refuses
    /// every operation with a reason; the agent keeps monitoring.
    ///
    /// # Errors
    /// [`PackageError::InvalidKey`] for a bad trusted key (fatal at agent start), or
    /// [`PackageError::UnsupportedPlatform`] off Linux.
    pub fn open(
        cfg: &PackagesSection,
        dirs: PackageDirs,
        deps: PackageDeps,
    ) -> Result<Self, PackageError> {
        if !cfg!(target_os = "linux") {
            return Err(PackageError::UnsupportedPlatform);
        }
        let keys = KeySet::from_section(cfg)?;
        let uid = layout::effective_uid();
        let mut fence = None;
        let mut staging = None;
        let prepared = layout::create_packages_dir(&dirs.packages_dir)
            .and_then(|()| layout::check_packages_dir(&dirs, uid))
            .and_then(|()| Staging::open(&dirs.packages_dir, Arc::clone(&deps.clock)));
        match prepared {
            Ok(opened) => staging = Some(opened),
            Err(error) => fence = Some(error),
        }
        let service = Self {
            dirs,
            limits: cfg.limits.clone(),
            allow_install: cfg.allow_install,
            keys,
            deps,
            staging,
            fence: Mutex::new(fence),
            ops: Mutex::new(()),
            skills_changed: Arc::new(tokio::sync::Notify::new()),
            uid,
            #[cfg(test)]
            faults: Mutex::new(Faults::default()),
        };
        if service.fence_error().is_none() {
            recover::run(&service);
        }
        Ok(service)
    }

    /// Signalled whenever enabling, disabling, installing or removing changed which package
    /// skills can run.
    #[must_use]
    pub fn skills_changed(&self) -> Arc<tokio::sync::Notify> {
        self.skills_changed.clone()
    }

    /// What this agent offers.
    #[must_use]
    pub fn surface(&self) -> PackageSurface {
        PackageSurface {
            list: true,
            mutate: self.allow_install,
        }
    }

    /// The staging area the transfer receiver writes into; `None` while the service is
    /// fenced (the packages directory is unusable), when transfers must be refused.
    #[must_use]
    pub fn staging(&self) -> Option<Arc<Staging>> {
        self.staging.clone()
    }

    /// Where this service sends audit events; the transfer receiver reports to the same sink.
    #[must_use]
    pub fn events(&self) -> Arc<dyn EventSink> {
        self.deps.events.clone()
    }

    /// The configured limits.
    #[must_use]
    pub fn limits(&self) -> &PackageLimits {
        &self.limits
    }

    /// Whether `allow_install` is on.
    #[must_use]
    pub fn install_allowed(&self) -> bool {
        self.allow_install && self.fence_error().is_none()
    }

    /// Whether the privilege boundary is in effect: only when the agent runs as the
    /// `domovoi-agent` account with the expected groups, not merely when it is not root.
    #[must_use]
    pub fn privilege_boundary(&self) -> PrivilegeBoundary {
        if self.deps.boundary_in_effect {
            PrivilegeBoundary::InEffect
        } else {
            PrivilegeBoundary::NotInEffect
        }
    }

    pub(crate) fn fence_error(&self) -> Option<PackageError> {
        self.fence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_fence(&self, error: PackageError) {
        *self.fence.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
    }

    /// The reason the service refuses everything, or an error when `packages_dir` stopped
    /// being safe since the last check (which fences the service).
    pub(crate) fn ready(&self) -> Result<(), PackageError> {
        if let Some(error) = self.fence_error() {
            return Err(error);
        }
        if let Err(error) = layout::check_packages_dir(&self.dirs, self.uid) {
            self.set_fence(error.clone());
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn lock_ops(&self) -> MutexGuard<'_, ()> {
        self.ops.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn registry_path(&self) -> PathBuf {
        self.dirs.packages_dir.join(layout::REGISTRY_FILE)
    }

    /// Reads the registry; a registry that cannot be read fences the service.
    pub(crate) fn read_registry(&self) -> Result<registry::Registry, PackageError> {
        registry::Registry::read(&self.registry_path()).inspect_err(|error| {
            self.set_fence(error.clone());
        })
    }

    pub(crate) fn staging_ref(&self) -> Result<&Arc<Staging>, PackageError> {
        self.staging.as_ref().ok_or_else(|| {
            PackageError::new(
                PackageFailureCode::PackagesDirUnsafe,
                "the packages directory is not usable",
            )
        })
    }

    /// Stops with an injected crash at `step` when a test asked for it.
    pub(crate) fn fault(&self, step: InstallStep) -> Result<(), PackageError> {
        #[cfg(test)]
        {
            let faults = *self.faults.lock().unwrap_or_else(PoisonError::into_inner);
            if faults.crash_after == Some(step) {
                return Err(PackageError::InjectedCrash);
            }
        }
        #[cfg(not(test))]
        let _ = step;
        Ok(())
    }

    pub(crate) fn durable_faults(&self) -> crate::fs_atomic::DurableFaults {
        #[cfg(test)]
        {
            self.faults
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .durable
        }
        #[cfg(not(test))]
        {
            crate::fs_atomic::DurableFaults::default()
        }
    }

    /// For the skill runner: refuses unless `name` is registered at `version`, its signer is
    /// trusted and not revoked now, and its files exist. Nothing calls it yet.
    ///
    /// # Errors
    /// [`ExecutionDenied`] with the reason.
    pub fn resolve_for_execution(
        &self,
        name: &domovoi_core::package::PackageName,
        version: &domovoi_core::package::PackageVersion,
    ) -> Result<ResolvedPackage, ExecutionDenied> {
        let registry = self
            .read_registry()
            .map_err(|_| ExecutionDenied::NotInstalled)?;
        if self.fence_error().is_some() {
            return Err(ExecutionDenied::NotInstalled);
        }
        let entry = registry.get(name).ok_or(ExecutionDenied::NotInstalled)?;
        if &entry.version != version {
            return Err(ExecutionDenied::VersionMismatch {
                installed: entry.version.clone(),
            });
        }
        let state = self.keys.state(&entry.key_id);
        if state != domovoi_core::package::KeyState::Trusted {
            return Err(ExecutionDenied::UntrustedKey {
                key_id: entry.key_id.clone(),
                state,
            });
        }
        let path = self
            .dirs
            .packages_dir
            .join(name.as_str())
            .join(version.as_str());
        if !path.is_dir() {
            return Err(ExecutionDenied::FilesMissing);
        }
        Ok(ResolvedPackage {
            name: name.clone(),
            version: version.clone(),
            path,
            key_id: entry.key_id.clone(),
            skills: entry.skills.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
