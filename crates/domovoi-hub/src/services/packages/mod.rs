//! The package use cases: importing and deleting signed packages, planning and running
//! install jobs, and reading what the hub holds and what its hosts report.
//!
//! Every write takes a [`Caller`]. A job is approved by the signed-in owner on the web
//! surface only, after their password and the package's typed name (ADR 0029); the approval
//! is made by [`crate::auth::approval::web_package_authorization`] and handed to the install
//! service, which never makes one.

mod error;
mod jobs;
mod view;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::{Clock, HostRepo, InstallRepo};
use domovoi_core::{HostName, HostPackage, PackageArtifact};
use domovoi_engine::packages::PackageHandle;

pub use error::{PASSWORD_REQUIRED_TEXT, PackageCommandError};
pub use jobs::{JobInput, JobsQuery, MAX_JOB_HOSTS, PlanInput, RetryInput};
pub use view::{JobStatus, JobView, job_status};

use super::{Caller, StepUp, Surface};
use crate::auth::{AuthError, AuthState};
use crate::packages::{
    ArchiveSource, ImportContext, ImportError, ImportOutcome, PackageImporter, StagedArchive,
};

/// What the hub holds and how it is limited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagesStatus {
    /// Whether packages can be imported and installed.
    pub enabled: bool,
    /// The largest archive accepted.
    pub max_artifact_bytes: u64,
    /// The most bytes of archives kept.
    pub max_store_bytes: u64,
    /// Bytes of archives the index accounts for.
    pub used_store_bytes: u64,
    /// How many packages are imported.
    pub artifact_count: u64,
    /// The owner keys: id and label. Never the keys themselves.
    pub owner_keys: Vec<(String, Option<String>)>,
    /// Revoked key ids.
    pub revoked: Vec<String>,
    /// How long an approval to install stays valid, in seconds.
    pub approval_window_secs: i64,
}

/// One imported package with the hosts that report it and whether its file is on disk.
#[derive(Debug, Clone)]
pub struct PackageDetail {
    /// The index row.
    pub artifact: PackageArtifact,
    /// Whether the archive file is in the store.
    pub file_present: bool,
    /// What the hub last saw of this version on each host, with the host's name.
    pub hosts: Vec<(HostPackage, HostName)>,
}

/// What the package use cases are built from.
pub struct PackageDeps {
    /// The install service.
    pub handle: PackageHandle,
    /// Import, delete and the file store.
    pub importer: Arc<PackageImporter>,
    /// Packages, jobs and inventories, read only.
    pub installs: Arc<dyn InstallRepo>,
    /// Hosts, read only.
    pub hosts: Arc<dyn HostRepo>,
    /// Asks for the password again.
    pub auth: Arc<AuthState>,
    /// Time, for the approval's timestamp.
    pub clock: Arc<dyn Clock>,
}

/// The package use cases.
#[derive(Clone)]
pub struct PackageCommands {
    pub(super) handle: PackageHandle,
    pub(super) importer: Arc<PackageImporter>,
    pub(super) installs: Arc<dyn InstallRepo>,
    pub(super) hosts: Arc<dyn HostRepo>,
    pub(super) auth: Arc<AuthState>,
    pub(super) clock: Arc<dyn Clock>,
}

impl PackageCommands {
    /// The use cases over `deps`.
    #[must_use]
    pub fn new(deps: PackageDeps) -> Self {
        Self {
            handle: deps.handle,
            importer: deps.importer,
            installs: deps.installs,
            hosts: deps.hosts,
            auth: deps.auth,
            clock: deps.clock,
        }
    }

    pub(super) fn clock_now(&self) -> jiff::Timestamp {
        self.clock.now()
    }

    /// Whether `[packages] enabled` is set.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.importer.config().enabled
    }

    pub(super) fn require_enabled(&self) -> Result<(), PackageCommandError> {
        if self.enabled() {
            Ok(())
        } else {
            Err(PackageCommandError::Disabled)
        }
    }

    /// Package writes are made in the web UI by a signed-in person; chat never reaches them.
    pub(super) fn web_session(caller: &Caller) -> Result<&[u8; 32], PackageCommandError> {
        match caller.surface() {
            Surface::Web { session_hash } => Ok(session_hash),
            Surface::Chat { .. } => Err(PackageCommandError::WebOnly),
        }
    }

    /// The hub's package limits, keys and how much of the store is used. Works while
    /// packages are disabled.
    ///
    /// # Errors
    /// A store failure.
    pub async fn status(&self) -> Result<PackagesStatus, PackageCommandError> {
        let config = self.importer.config();
        let rows = self.installs.artifacts().await?;
        let mut files = BTreeMap::new();
        for row in &rows {
            files.insert(row.archive_sha256, row.archive_size);
        }
        let keys = self.importer.keys();
        Ok(PackagesStatus {
            enabled: config.enabled,
            max_artifact_bytes: config.max_artifact_bytes,
            max_store_bytes: config.max_store_bytes,
            used_store_bytes: files.values().sum(),
            artifact_count: rows.len() as u64,
            owner_keys: keys
                .owners()
                .map(|(id, owner)| (id.to_string(), owner.label.clone()))
                .collect(),
            revoked: keys.revoked().map(ToString::to_string).collect(),
            approval_window_secs: config.approval_window.as_signed().as_secs(),
        })
    }

    /// Every imported package, by name and version.
    ///
    /// # Errors
    /// A store failure.
    pub async fn list(&self) -> Result<Vec<PackageArtifact>, PackageCommandError> {
        let mut rows = self.installs.artifacts().await?;
        rows.sort_by(|a, b| {
            (a.name.as_str(), a.version.as_str()).cmp(&(b.name.as_str(), b.version.as_str()))
        });
        Ok(rows)
    }

    async fn artifact(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<PackageArtifact, PackageCommandError> {
        self.installs
            .artifact(name, version)
            .await?
            .ok_or(PackageCommandError::Import(ImportError::NoSuchPackage))
    }

    /// One imported package, the state of its file and the hosts known to have it.
    ///
    /// # Errors
    /// No such package, or a store failure.
    pub async fn detail(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<PackageDetail, PackageCommandError> {
        let artifact = self.artifact(name, version).await?;
        let file_present = self
            .importer
            .store()
            .contains(&artifact.archive_sha256)
            .await;
        let mut hosts = Vec::new();
        let mut seen = BTreeSet::new();
        for row in self.installs.hosts_with(name, version).await? {
            if !seen.insert(row.host) {
                continue;
            }
            if let Some(host) = self.hosts.get_host(row.host).await? {
                hosts.push((row, host.name));
            }
        }
        Ok(PackageDetail {
            artifact,
            file_present,
            hosts,
        })
    }

    fn context(caller: &Caller) -> ImportContext {
        ImportContext {
            actor: caller.actor(),
            audit_rejections: true,
        }
    }

    /// Whether `caller` may import now: a web caller, with packages enabled. Checked before
    /// an upload is read, so a refused request costs nothing.
    ///
    /// # Errors
    /// [`PackageCommandError::WebOnly`] or [`PackageCommandError::Disabled`].
    pub fn stage_allowed(&self, caller: &Caller) -> Result<(), PackageCommandError> {
        Self::web_session(caller)?;
        self.require_enabled()
    }

    /// Streams an archive into the store's staging area.
    ///
    /// # Errors
    /// [`PackageCommandError::Disabled`], [`PackageCommandError::WebOnly`] or the import
    /// refusal.
    pub async fn stage(
        &self,
        caller: &Caller,
        source: &mut dyn ArchiveSource,
    ) -> Result<StagedArchive, PackageCommandError> {
        Self::web_session(caller)?;
        self.require_enabled()?;
        Ok(self.importer.stage(&Self::context(caller), source).await?)
    }

    /// Checks a staged archive against its signature file and stores it.
    ///
    /// # Errors
    /// The import refusal; the staged file is deleted either way.
    pub async fn finish(
        &self,
        caller: &Caller,
        staged: StagedArchive,
        signature: &[u8],
    ) -> Result<ImportOutcome, PackageCommandError> {
        Ok(self
            .importer
            .finish(&Self::context(caller), staged, signature)
            .await?)
    }

    /// Removes an imported package. Copies already installed on hosts are untouched.
    ///
    /// # Errors
    /// [`PackageCommandError::Disabled`], no such package, or the package is in use.
    pub async fn delete(
        &self,
        caller: &Caller,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<PackageArtifact, PackageCommandError> {
        Self::web_session(caller)?;
        self.require_enabled()?;
        let artifact = self.artifact(name, version).await?;
        Ok(self.importer.delete(&caller.actor(), artifact.id).await?)
    }

    /// Asks for the password again. It shares the login limiter.
    pub(super) async fn step_up(
        &self,
        caller: &Caller,
        proof: StepUp<'_>,
    ) -> Result<(), PackageCommandError> {
        let Some(password) = proof.password else {
            return Err(PackageCommandError::PasswordRequired);
        };
        match self
            .auth
            .confirm_password(proof.ip, caller.user(), password)
            .await
        {
            Ok(()) => Ok(()),
            Err(AuthError::LockedOut { retry_after_secs }) => {
                Err(PackageCommandError::TooMany { retry_after_secs })
            }
            Err(_) => Err(PackageCommandError::BadPassword),
        }
    }
}
