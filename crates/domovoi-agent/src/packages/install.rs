//! `agent.package.install`: verify, audit, extract, place and register one package.
//!
//! The registry write is the commit point. Every step before it leaves nothing under
//! `packages_dir/<name>/` that recovery would not sweep, and every step after it is cleanup.

use std::path::Path;

use domovoi_core::package::{
    MAX_INSTALLED_PACKAGES, MAX_SKILLS_PER_PACKAGE, PackageFailureCode, PackageManifest,
    PackageName, SignatureFile,
};
use domovoi_proto::PROTOCOL_VERSION;
use domovoi_proto::messages::TransferKind;
use domovoi_runner::SkillCatalog;
use uuid::Uuid;

use super::error::{PackageError, quote};
use super::events::{EventKind, PackageEvent};
use super::registry::{Registry, RegistryEntry, WriteFailure};
use super::{
    InstallOutcome, InstallRequest, InstallStep, PackageService, archive, extract, verify,
};
use crate::fs_atomic::sync_dir;

/// Whether `error` is the test hook that simulates a crash (no cleanup may follow it).
fn is_crash(error: &PackageError) -> bool {
    #[cfg(test)]
    {
        matches!(error, PackageError::InjectedCrash)
    }
    #[cfg(not(test))]
    {
        let _ = error;
        false
    }
}

fn fail(code: PackageFailureCode, detail: impl Into<String>) -> PackageError {
    PackageError::new(code, detail)
}

impl PackageService {
    /// Installs the package described by `req`, using the two stored transfers.
    ///
    /// Both transfers are consumed whatever the outcome once they are claimed. Installing
    /// never enables a package and never runs anything from it.
    ///
    /// # Errors
    /// A [`PackageError`] whose code is one of the stable package failure codes.
    pub fn install(
        &self,
        execution_id: Uuid,
        req: InstallRequest,
    ) -> Result<InstallOutcome, PackageError> {
        let _guard = self.lock_ops();
        let outcome = self.install_locked(execution_id, &req);
        if let Err(error) = &outcome
            && !is_crash(error)
        {
            self.deps.events.emit(
                PackageEvent::new(EventKind::Rejected)
                    .execution(execution_id)
                    .package(&req.package, &req.version)
                    .code(error.code()),
            );
        }
        outcome
    }

    fn install_locked(
        &self,
        exec: Uuid,
        req: &InstallRequest,
    ) -> Result<InstallOutcome, PackageError> {
        self.ready()?;
        if !self.allow_install {
            return Err(fail(
                PackageFailureCode::PackagesDisabled,
                "package installation is not allowed on this agent",
            ));
        }
        let staging = self.staging_ref()?;
        let artifact = staging.claim(req.artifact_transfer, TransferKind::PackageArtifact)?;
        let signature = match staging.claim(req.signature_transfer, TransferKind::PackageManifest) {
            Ok(claimed) => claimed,
            Err(error) => {
                staging.delete_claimed(&artifact);
                return Err(error);
            }
        };
        let result = self.install_claimed(exec, req, &artifact.path, &signature.path);
        if result.as_ref().err().is_none_or(|error| !is_crash(error)) {
            staging.delete_claimed(&artifact);
            staging.delete_claimed(&signature);
            staging.remove_exec_dir(exec);
        }
        result
    }

    fn install_claimed(
        &self,
        exec: Uuid,
        req: &InstallRequest,
        artifact: &Path,
        signature: &Path,
    ) -> Result<InstallOutcome, PackageError> {
        let signature_bytes = std::fs::read(signature)
            .map_err(|error| PackageError::io("reading the signature file", &error))?;
        let sf = verify::parse_signature_file(&signature_bytes)?;
        verify::same_identity(&sf, &req.package, &req.version)?;
        let key = self.keys.lookup(&sf.key_id)?;
        verify::signature(key, &sf)?;
        let file = std::fs::File::open(artifact)
            .map_err(|error| PackageError::io("opening the archive", &error))?;
        verify::check_digest(&verify::sha256_stream(file)?, &sf)?;
        self.deps.events.emit(
            PackageEvent::new(EventKind::Verified)
                .execution(exec)
                .package(&req.package, &req.version)
                .key(&sf.key_id)
                .digest(&sf.archive_sha256),
        );
        let clock = &*self.deps.clock;
        let audited = archive::audit_file(
            artifact,
            &req.package,
            &self.limits,
            clock,
            &sf.archive_sha256,
        )?;
        let manifest = verify::parse_manifest(&audited.manifest_bytes)?;
        verify::manifest_identity(&manifest, &req.package, &req.version)?;
        verify::compat(
            &manifest,
            &PROTOCOL_VERSION.to_string(),
            &self.deps.platform,
        )?;

        let registry = self.read_registry()?;
        let replaced = match registry.get(&req.package) {
            Some(existing) if existing.version == req.version => {
                if existing.archive_sha256 == sf.archive_sha256 {
                    return self.already_installed(existing, &sf, &registry);
                }
                return Err(fail(
                    PackageFailureCode::VersionConflict,
                    "this version is installed from a different archive",
                ));
            }
            Some(existing) => Some(existing.version.clone()),
            None if registry.len() >= MAX_INSTALLED_PACKAGES => {
                return Err(fail(
                    PackageFailureCode::TooManyPackages,
                    "the maximum number of packages is installed",
                ));
            }
            None => None,
        };
        let needed = self.limits.uncompressed_bytes.saturating_mul(2);
        let free = self
            .deps
            .space
            .free_bytes(&self.dirs.packages_dir)
            .map_err(|error| PackageError::io("checking free space", &error))?;
        if free < needed {
            return Err(fail(
                PackageFailureCode::NoSpace,
                format!("{needed} bytes are needed and {free} are free"),
            ));
        }
        self.fault(InstallStep::AfterAudit)?;

        let staging = self.staging_ref()?;
        staging.make_exec_dir(exec)?;
        let tree = staging.exec_tree(exec);
        extract::run(
            artifact,
            &req.package,
            &self.limits,
            clock,
            &sf.archive_sha256,
            &tree,
        )?;
        self.fault(InstallStep::AfterExtract)?;
        let skills = self.validate_tree(&tree, &manifest, &registry)?;

        let package_dir = self.dirs.packages_dir.join(req.package.as_str());
        let dest = package_dir.join(req.version.as_str());
        super::layout::ensure_dir(&package_dir, super::layout::DIR_MODE)?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|error| PackageError::io("removing a stale version", &error))?;
        }
        std::fs::rename(&tree, &dest)
            .map_err(|error| PackageError::io("placing the package", &error))?;
        sync_dir(&package_dir).map_err(|error| PackageError::io("syncing the package", &error))?;
        self.fault(InstallStep::AfterRename)?;

        let installed_at = clock.now();
        let entry = RegistryEntry {
            name: req.package.clone(),
            version: req.version.clone(),
            key_id: sf.key_id.clone(),
            archive_sha256: sf.archive_sha256,
            installed_at,
            execution_id: exec,
            skills: skills.clone(),
        };
        self.commit(&registry.with(entry), exec, &req.package, &dest)?;
        self.fault(InstallStep::AfterRegistry)?;

        if let Some(old) = &replaced {
            let _ = std::fs::remove_dir_all(package_dir.join(old.as_str()));
        }
        self.fault(InstallStep::AfterOldRemoved)?;
        self.deps.events.emit(
            PackageEvent::new(EventKind::Installed)
                .execution(exec)
                .package(&req.package, &req.version)
                .key(&sf.key_id)
                .digest(&sf.archive_sha256),
        );
        Ok(InstallOutcome {
            package: req.package.clone(),
            version: req.version.clone(),
            key_id: sf.key_id,
            archive_sha256: sf.archive_sha256,
            installed_at,
            skills,
            replaced_version: replaced,
            already_installed: false,
            signer_updated: false,
            privilege_boundary: self.privilege_boundary(),
        })
    }

    /// Writes `next` as the registry; on a failure the placed tree at `dest` is removed.
    fn commit(
        &self,
        next: &Registry,
        exec: Uuid,
        name: &PackageName,
        dest: &Path,
    ) -> Result<(), PackageError> {
        match next.write(&self.registry_path(), self.durable_faults()) {
            Ok(()) => Ok(()),
            Err(WriteFailure::BeforeCommit(error)) => {
                let _ = std::fs::remove_dir_all(dest);
                Err(error)
            }
            Err(WriteFailure::Indeterminate(error)) => {
                let landed = Registry::read(&self.registry_path())
                    .ok()
                    .and_then(|read| read.get(name).map(|entry| entry.execution_id == exec))
                    .unwrap_or(false);
                if landed {
                    Ok(())
                } else {
                    let _ = std::fs::remove_dir_all(dest);
                    Err(error)
                }
            }
        }
    }

    /// The same archive is registered already: only a changed signer is recorded.
    fn already_installed(
        &self,
        existing: &RegistryEntry,
        sf: &SignatureFile,
        registry: &Registry,
    ) -> Result<InstallOutcome, PackageError> {
        let signer_updated = existing.key_id != sf.key_id;
        if signer_updated {
            let mut updated = existing.clone();
            updated.key_id = sf.key_id.clone();
            registry
                .with(updated)
                .write(&self.registry_path(), self.durable_faults())
                .map_err(|failure| match failure {
                    WriteFailure::BeforeCommit(error) | WriteFailure::Indeterminate(error) => error,
                })?;
        }
        Ok(InstallOutcome {
            package: existing.name.clone(),
            version: existing.version.clone(),
            key_id: sf.key_id.clone(),
            archive_sha256: existing.archive_sha256,
            installed_at: existing.installed_at,
            skills: existing.skills.clone(),
            replaced_version: None,
            already_installed: true,
            signer_updated,
            privilege_boundary: self.privilege_boundary(),
        })
    }

    /// Checks the extracted tree's skills and returns their sorted names.
    fn validate_tree(
        &self,
        tree: &Path,
        manifest: &PackageManifest,
        registry: &Registry,
    ) -> Result<Vec<String>, PackageError> {
        let skills_root = tree.join("skills");
        if !skills_root.is_dir() {
            return Err(fail(
                PackageFailureCode::NoSkills,
                "the package has no skills",
            ));
        }
        let (catalog, issues) = SkillCatalog::load(&skills_root);
        if let Some(issue) = issues.first() {
            return Err(fail(
                PackageFailureCode::SkillsInvalid,
                format!("a skill is invalid: {}", quote(&issue.message)),
            ));
        }
        let summaries = catalog.summaries();
        if summaries.is_empty() {
            return Err(fail(
                PackageFailureCode::NoSkills,
                "the package has no skills",
            ));
        }
        if summaries.len() > MAX_SKILLS_PER_PACKAGE {
            return Err(fail(
                PackageFailureCode::SkillsInvalid,
                "the package has too many skills",
            ));
        }
        let mut names: Vec<String> = Vec::with_capacity(summaries.len());
        for summary in &summaries {
            if catalog
                .get(&summary.name)
                .is_some_and(|skill| skill.manifest().run_as.is_some())
            {
                return Err(fail(
                    PackageFailureCode::SkillRunAsForbidden,
                    format!("skill {} sets run_as", quote(&summary.name.to_string())),
                ));
            }
            names.push(summary.name.to_string());
        }
        names.sort();
        self.check_name_conflicts(&names, &manifest.name, registry)?;
        Ok(names)
    }

    fn check_name_conflicts(
        &self,
        names: &[String],
        package: &PackageName,
        registry: &Registry,
    ) -> Result<(), PackageError> {
        let (owner, _) = SkillCatalog::load(&self.dirs.skills_dir);
        let owned: Vec<String> = owner
            .summaries()
            .iter()
            .map(|summary| summary.name.to_string())
            .collect();
        for name in names {
            let clash_owner =
                owned.contains(name) || self.dirs.skills_dir.join(name).symlink_metadata().is_ok();
            let clash_package = registry
                .entries()
                .iter()
                .any(|entry| &entry.name != package && entry.skills.contains(name));
            if clash_owner || clash_package {
                return Err(fail(
                    PackageFailureCode::SkillConflict,
                    format!("skill {} already exists on this host", quote(name)),
                ));
            }
        }
        Ok(())
    }
}
