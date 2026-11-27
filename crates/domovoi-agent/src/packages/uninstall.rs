//! `agent.package.uninstall`: drop a package from the registry, then delete its files.
//!
//! The registry write is the commit point. Files left behind by a crash after it are swept at
//! the next start. Removal does not need the signer to be trusted.

use domovoi_core::package::PackageFailureCode;

use super::error::PackageError;
use super::events::{EventKind, PackageEvent};
use super::registry::{Registry, WriteFailure};
use super::{InstallStep, PackageService, UninstallOutcome, UninstallRequest};

impl PackageService {
    /// Removes the installed package named by `req`.
    ///
    /// An absent package is success with `removed = false`.
    ///
    /// # Errors
    /// `version_conflict` when another version is installed, `packages_disabled`, or a
    /// storage or fence failure.
    pub fn uninstall(&self, req: UninstallRequest) -> Result<UninstallOutcome, PackageError> {
        let _guard = self.lock_ops();
        self.ready()?;
        if !self.allow_install {
            return Err(PackageError::new(
                PackageFailureCode::PackagesDisabled,
                "package installation is not allowed on this agent",
            ));
        }
        let registry = self.read_registry()?;
        let absent = UninstallOutcome {
            package: req.package.clone(),
            version: req.version.clone(),
            removed: false,
            privilege_boundary: self.privilege_boundary(),
        };
        let Some(existing) = registry.get(&req.package) else {
            return Ok(absent);
        };
        if existing.version != req.version {
            return Err(PackageError::new(
                PackageFailureCode::VersionConflict,
                "a different version of this package is installed",
            ));
        }
        self.fault(InstallStep::BeforeUninstallRegistry)?;
        let path = self.registry_path();
        match registry
            .without(&req.package)
            .write(&path, self.durable_faults())
        {
            Ok(()) => {}
            Err(WriteFailure::BeforeCommit(error)) => return Err(error),
            Err(WriteFailure::Indeterminate(error)) => {
                let gone = Registry::read(&path).is_ok_and(|read| read.get(&req.package).is_none());
                if !gone {
                    return Err(error);
                }
            }
        }
        self.forget_enabled(&req.package);
        self.skills_changed.notify_one();
        self.fault(InstallStep::AfterUninstallRegistry)?;
        let package_dir = self.dirs.packages_dir.join(req.package.as_str());
        let _ = std::fs::remove_dir_all(package_dir.join(req.version.as_str()));
        let _ = std::fs::remove_dir(&package_dir);
        self.deps.events.emit(
            PackageEvent::new(EventKind::Uninstalled)
                .package(&req.package, &req.version)
                .key(&existing.key_id)
                .digest(&existing.archive_sha256),
        );
        Ok(UninstallOutcome {
            removed: true,
            ..absent
        })
    }
}
