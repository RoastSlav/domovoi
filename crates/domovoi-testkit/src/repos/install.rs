//! The in-memory install repository: reads only. Writes arrive through the unit of work.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::install::{HostPackage, InstallJob, InstallTarget, PackageArtifact};
use domovoi_core::package::{ArchiveDigest, PackageName, PackageVersion};
use domovoi_core::ports::{InstallJobFilter, InstallRepo, Page, Paged, StoreError};
use domovoi_core::{HostId, InstallJobId, InstallTargetId, PackageArtifactId};

use super::{InMemoryStore, paginate};

/// Everything the install aggregates keep in memory.
#[derive(Default, Clone)]
pub(super) struct InstallState {
    /// Imported artifacts by id.
    pub(super) artifacts: BTreeMap<PackageArtifactId, PackageArtifact>,
    /// Jobs by id.
    pub(super) jobs: BTreeMap<InstallJobId, InstallJob>,
    /// Targets by id.
    pub(super) targets: BTreeMap<InstallTargetId, InstallTarget>,
    /// Inventory rows by host and package name.
    pub(super) packages: BTreeMap<(HostId, PackageName), HostPackage>,
}

impl InstallState {
    /// Targets of `job`, ordered by host then attempt.
    fn targets_of(&self, job: InstallJobId) -> Vec<InstallTarget> {
        let mut targets: Vec<InstallTarget> = self
            .targets
            .values()
            .filter(|t| t.job == job)
            .cloned()
            .collect();
        targets.sort_by_key(|t| (t.host.to_string(), t.attempt));
        targets
    }

    /// Targets that are queued, transferring or dispatched, oldest first.
    fn non_terminal(&self) -> Vec<InstallTarget> {
        let mut targets: Vec<InstallTarget> = self
            .targets
            .values()
            .filter(|t| !t.status.is_terminal())
            .cloned()
            .collect();
        targets.sort_by_key(|t| (t.queued_at, t.id.to_string()));
        targets
    }

    /// Whether any non-terminal target of `job` exists.
    fn is_active(&self, job: InstallJobId) -> bool {
        self.targets
            .values()
            .any(|t| t.job == job && !t.status.is_terminal())
    }
}

#[async_trait]
impl InstallRepo for InMemoryStore {
    async fn artifacts(&self) -> Result<Vec<PackageArtifact>, StoreError> {
        let state = self.begin()?;
        let mut all: Vec<PackageArtifact> = state.install.artifacts.values().cloned().collect();
        all.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        Ok(all)
    }

    async fn artifact(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Option<PackageArtifact>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .install
            .artifacts
            .values()
            .find(|a| a.name == *name && a.version == *version)
            .cloned())
    }

    async fn artifact_by_digest(
        &self,
        digest: &ArchiveDigest,
    ) -> Result<Vec<PackageArtifact>, StoreError> {
        let state = self.begin()?;
        let mut all: Vec<PackageArtifact> = state
            .install
            .artifacts
            .values()
            .filter(|a| a.archive_sha256 == *digest)
            .cloned()
            .collect();
        all.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        Ok(all)
    }

    async fn job(&self, id: InstallJobId) -> Result<Option<InstallJob>, StoreError> {
        Ok(self.begin()?.install.jobs.get(&id).cloned())
    }

    async fn jobs(
        &self,
        filter: &InstallJobFilter,
        page: &Page,
    ) -> Result<Paged<InstallJob>, StoreError> {
        let state = self.begin()?;
        let items = state
            .install
            .jobs
            .values()
            .filter(|job| {
                filter.name.as_ref().is_none_or(|n| job.name == *n)
                    && filter.host.is_none_or(|host| {
                        state
                            .install
                            .targets
                            .values()
                            .any(|t| t.job == job.id && t.host == host)
                    })
                    && filter
                        .active
                        .is_none_or(|active| state.install.is_active(job.id) == active)
            })
            .map(|job| {
                (
                    (
                        i128::from(job.created_at.as_millisecond()),
                        job.id.to_string(),
                    ),
                    job.clone(),
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn targets(&self, job: InstallJobId) -> Result<Vec<InstallTarget>, StoreError> {
        Ok(self.begin()?.install.targets_of(job))
    }

    async fn target(&self, id: InstallTargetId) -> Result<Option<InstallTarget>, StoreError> {
        Ok(self.begin()?.install.targets.get(&id).cloned())
    }

    async fn non_terminal_targets(&self) -> Result<Vec<InstallTarget>, StoreError> {
        Ok(self.begin()?.install.non_terminal())
    }

    async fn queued_targets(&self) -> Result<Vec<InstallTarget>, StoreError> {
        let mut all = self.begin()?.install.non_terminal();
        all.retain(|t| t.status == domovoi_core::TargetStatus::Queued);
        Ok(all)
    }

    async fn latest_target(
        &self,
        job: InstallJobId,
        host: HostId,
    ) -> Result<Option<InstallTarget>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .install
            .targets
            .values()
            .filter(|t| t.job == job && t.host == host)
            .max_by_key(|t| t.attempt)
            .cloned())
    }

    async fn inventory(&self, host: HostId) -> Result<Vec<HostPackage>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .install
            .packages
            .iter()
            .filter(|((h, _), _)| *h == host)
            .map(|(_, row)| row.clone())
            .collect())
    }

    async fn hosts_with(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Vec<HostPackage>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<HostPackage> = state
            .install
            .packages
            .values()
            .filter(|row| row.name == *name && row.version == *version)
            .cloned()
            .collect();
        rows.sort_by_key(|row| row.host.to_string());
        Ok(rows)
    }
}
