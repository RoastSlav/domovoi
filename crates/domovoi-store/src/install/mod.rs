//! Package artifacts, install jobs and host inventories: row codecs, write application and
//! retention. The read port is implemented in `repos/install.rs`.

mod apply;
pub(crate) mod codec;

use domovoi_core::ports::StoreError;
use jiff::{SignedDuration, Timestamp};

pub(crate) use apply::apply;

use crate::codec::ms;
use crate::db::Store;
use crate::error::db;

/// Deletes a job once every one of its targets is terminal and finished before the cutoff.
/// The targets go with it; artifacts, inventory and the audit log are untouched.
const DELETE_OLD_JOBS: &str = "DELETE FROM install_jobs WHERE created_at < ?1 \
    AND NOT EXISTS (SELECT 1 FROM install_job_targets t WHERE t.job_id = install_jobs.id \
        AND (t.status IN ('queued', 'transferring', 'dispatched') \
             OR coalesce(t.finished_at, t.queued_at) >= ?1))";

impl Store {
    /// Deletes install jobs whose targets are all terminal and older than `keep` at `now`,
    /// with their targets. Returns how many jobs were deleted.
    ///
    /// A job with a queued, transferring or dispatched target is never deleted.
    pub async fn purge_install_jobs(
        &self,
        now: Timestamp,
        keep: SignedDuration,
    ) -> Result<u64, StoreError> {
        let before = ms(now.checked_sub(keep).unwrap_or(Timestamp::MIN));
        let deleted = sqlx::query(DELETE_OLD_JOBS)
            .bind(before)
            .execute(&self.write)
            .await
            .map_err(db)?
            .rows_affected();
        tracing::info!(deleted, "purged old install jobs");
        Ok(deleted)
    }
}
