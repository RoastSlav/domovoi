//! Reading package artifacts, install jobs and host inventories. There is no write method:
//! they change only through the unit of work.

use async_trait::async_trait;

use super::{Page, Paged, StoreError};
use crate::id::{HostId, InstallJobId, InstallTargetId};
use crate::install::{HostPackage, InstallJob, InstallTarget, PackageArtifact};
use crate::package::{ArchiveDigest, PackageName, PackageVersion};

/// Which jobs a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallJobFilter {
    /// Only jobs that have a target on this host.
    pub host: Option<HostId>,
    /// Only jobs for this package.
    pub name: Option<PackageName>,
    /// `Some(true)` keeps jobs with a non-terminal target, `Some(false)` jobs whose targets
    /// are all terminal.
    pub active: Option<bool>,
}

/// Stored package artifacts, install jobs, targets and host inventories, read-only.
#[async_trait]
pub trait InstallRepo: Send + Sync {
    /// Every imported artifact, ordered by name then version.
    async fn artifacts(&self) -> Result<Vec<PackageArtifact>, StoreError>;

    /// The artifact for `name` and `version`.
    async fn artifact(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Option<PackageArtifact>, StoreError>;

    /// Every artifact whose archive has `digest`; one file can back more than one row.
    async fn artifact_by_digest(
        &self,
        digest: &ArchiveDigest,
    ) -> Result<Vec<PackageArtifact>, StoreError>;

    /// The job with `id`.
    async fn job(&self, id: InstallJobId) -> Result<Option<InstallJob>, StoreError>;

    /// One page of jobs matching `filter`, newest first.
    async fn jobs(
        &self,
        filter: &InstallJobFilter,
        page: &Page,
    ) -> Result<Paged<InstallJob>, StoreError>;

    /// Every target of `job`, ordered by host then attempt.
    async fn targets(&self, job: InstallJobId) -> Result<Vec<InstallTarget>, StoreError>;

    /// The target with `id`.
    async fn target(&self, id: InstallTargetId) -> Result<Option<InstallTarget>, StoreError>;

    /// Every target that is queued, transferring or dispatched, oldest `queued_at` first.
    async fn non_terminal_targets(&self) -> Result<Vec<InstallTarget>, StoreError>;

    /// Every queued target, oldest `queued_at` first.
    async fn queued_targets(&self) -> Result<Vec<InstallTarget>, StoreError>;

    /// The newest attempt of `job` on `host`.
    async fn latest_target(
        &self,
        job: InstallJobId,
        host: HostId,
    ) -> Result<Option<InstallTarget>, StoreError>;

    /// What `host` has installed, as last observed, ordered by package name.
    async fn inventory(&self, host: HostId) -> Result<Vec<HostPackage>, StoreError>;

    /// The inventory rows of every host that has `name` at `version`.
    async fn hosts_with(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Vec<HostPackage>, StoreError>;
}
