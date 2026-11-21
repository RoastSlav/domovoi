//! Reading package artifacts, install jobs, targets and host inventories.

use async_trait::async_trait;
use domovoi_core::install::{HostPackage, InstallJob, InstallTarget, PackageArtifact};
use domovoi_core::package::{ArchiveDigest, PackageName, PackageVersion};
use domovoi_core::ports::{InstallJobFilter, InstallRepo, Page, Paged, StoreError};
use domovoi_core::{HostId, InstallJobId, InstallTargetId};

use crate::codec::{Cursor, fetch_limit, ms, paginate};
use crate::db::Store;
use crate::error::db;
use crate::install::codec::{
    ArtifactRow, JobRow, PackageRow, TargetRow, artifact_columns, job_columns, package_columns,
    target_columns,
};

#[async_trait]
impl InstallRepo for Store {
    async fn artifacts(&self) -> Result<Vec<PackageArtifact>, StoreError> {
        const SQL: &str = concat!(artifact_columns!(), " ORDER BY name, version");
        let rows: Vec<ArtifactRow> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(ArtifactRow::into_artifact).collect()
    }

    async fn artifact(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Option<PackageArtifact>, StoreError> {
        const SQL: &str = concat!(artifact_columns!(), " WHERE name = ? AND version = ?");
        let row: Option<ArtifactRow> = sqlx::query_as(SQL)
            .bind(name.as_str())
            .bind(version.as_str())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ArtifactRow::into_artifact).transpose()
    }

    async fn artifact_by_digest(
        &self,
        digest: &ArchiveDigest,
    ) -> Result<Vec<PackageArtifact>, StoreError> {
        const SQL: &str = concat!(
            artifact_columns!(),
            " WHERE archive_sha256 = ? ORDER BY name, version"
        );
        let rows: Vec<ArtifactRow> = sqlx::query_as(SQL)
            .bind(digest.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(ArtifactRow::into_artifact).collect()
    }

    async fn job(&self, id: InstallJobId) -> Result<Option<InstallJob>, StoreError> {
        const SQL: &str = concat!(job_columns!(), " WHERE id = ?");
        let row: Option<JobRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(JobRow::into_job).transpose()
    }

    async fn jobs(
        &self,
        filter: &InstallJobFilter,
        page: &Page,
    ) -> Result<Paged<InstallJob>, StoreError> {
        const SQL: &str = concat!(
            job_columns!(),
            " WHERE (?1 IS NULL OR EXISTS (SELECT 1 FROM install_job_targets t \
                 WHERE t.job_id = install_jobs.id AND t.host_id = ?1)) \
             AND (?2 IS NULL OR name = ?2) \
             AND (?3 IS NULL OR ?3 = EXISTS (SELECT 1 FROM install_job_targets t \
                 WHERE t.job_id = install_jobs.id \
                 AND t.status IN ('queued', 'transferring', 'dispatched'))) \
             AND (?4 IS NULL OR created_at < ?4 OR (created_at = ?4 AND id < ?5)) \
             ORDER BY created_at DESC, id DESC LIMIT ?6"
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<JobRow> = sqlx::query_as(SQL)
            .bind(filter.host.map(|h| h.to_string()))
            .bind(filter.name.as_ref().map(|n| n.as_str().to_owned()))
            .bind(filter.active.map(i64::from))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let jobs = rows
            .into_iter()
            .map(JobRow::into_job)
            .collect::<Result<Vec<_>, _>>()?;
        let (items, next_cursor) = paginate(jobs, limit, |job| Cursor {
            key: ms(job.created_at),
            id: job.id.to_string(),
        });
        Ok(Paged { items, next_cursor })
    }

    async fn targets(&self, job: InstallJobId) -> Result<Vec<InstallTarget>, StoreError> {
        const SQL: &str = concat!(
            target_columns!(),
            " WHERE job_id = ? ORDER BY host_id, attempt"
        );
        let rows: Vec<TargetRow> = sqlx::query_as(SQL)
            .bind(job.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(TargetRow::into_target).collect()
    }

    async fn target(&self, id: InstallTargetId) -> Result<Option<InstallTarget>, StoreError> {
        const SQL: &str = concat!(target_columns!(), " WHERE id = ?");
        let row: Option<TargetRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(TargetRow::into_target).transpose()
    }

    async fn non_terminal_targets(&self) -> Result<Vec<InstallTarget>, StoreError> {
        const SQL: &str = concat!(
            target_columns!(),
            " WHERE status IN ('queued', 'transferring', 'dispatched') \
             ORDER BY queued_at, id"
        );
        let rows: Vec<TargetRow> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(TargetRow::into_target).collect()
    }

    async fn queued_targets(&self) -> Result<Vec<InstallTarget>, StoreError> {
        const SQL: &str = concat!(
            target_columns!(),
            " WHERE status = 'queued' ORDER BY queued_at, id"
        );
        let rows: Vec<TargetRow> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(TargetRow::into_target).collect()
    }

    async fn latest_target(
        &self,
        job: InstallJobId,
        host: HostId,
    ) -> Result<Option<InstallTarget>, StoreError> {
        const SQL: &str = concat!(
            target_columns!(),
            " WHERE job_id = ? AND host_id = ? ORDER BY attempt DESC LIMIT 1"
        );
        let row: Option<TargetRow> = sqlx::query_as(SQL)
            .bind(job.to_string())
            .bind(host.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(TargetRow::into_target).transpose()
    }

    async fn inventory(&self, host: HostId) -> Result<Vec<HostPackage>, StoreError> {
        const SQL: &str = concat!(package_columns!(), " WHERE host_id = ? ORDER BY name");
        let rows: Vec<PackageRow> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(PackageRow::into_package).collect()
    }

    async fn hosts_with(
        &self,
        name: &PackageName,
        version: &PackageVersion,
    ) -> Result<Vec<HostPackage>, StoreError> {
        const SQL: &str = concat!(
            package_columns!(),
            " WHERE name = ? AND version = ? ORDER BY host_id"
        );
        let rows: Vec<PackageRow> = sqlx::query_as(SQL)
            .bind(name.as_str())
            .bind(version.as_str())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(PackageRow::into_package).collect()
    }
}
