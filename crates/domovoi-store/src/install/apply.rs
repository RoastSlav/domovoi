//! Applying the install aggregate's writes on one SQLite connection.
//!
//! Every rule is checked in Rust first so a refusal is a typed [`CommitError`]; the unique
//! indexes in the migration are the backstop for anyone writing raw SQL.

use domovoi_core::install::{
    ArtifactGuard, HostPackage, InstallJob, InstallTarget, PackageArtifact, TargetGuard,
};
use domovoi_core::package::PackageName;
use domovoi_core::ports::{AggregateRef, CommitError, Expectation, InstallWrite, Observed};
use domovoi_core::{HostId, InstallJobId, InstallTargetId, PackageArtifactId};
use sqlx::SqliteConnection;

use super::codec::TargetBinds;
use crate::codec::{int, json, ms, name, uint};
use crate::uow::{backend, map_store};

/// The statuses a target has while it still holds its host.
const ACTIVE: &str = "('queued', 'transferring', 'dispatched')";

fn refuse(message: String) -> CommitError {
    CommitError::Constraint(message)
}

/// Applies one install write.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &InstallWrite,
) -> Result<(), CommitError> {
    match write {
        InstallWrite::ImportArtifact(artifact) => import_artifact(conn, artifact).await,
        InstallWrite::DeleteArtifact { id, expected } => delete_artifact(conn, *id, expected).await,
        InstallWrite::CreateJob { job, targets } => create_job(conn, job, targets).await,
        InstallWrite::NewAttempt(target) => insert_target(conn, target).await,
        InstallWrite::Target { target, expected } => update_target(conn, target, *expected).await,
        InstallWrite::Inventory {
            host,
            entries,
            replace,
        } => inventory(conn, *host, entries, *replace).await,
        InstallWrite::RemoveInventory { host, name } => remove_inventory(conn, *host, name).await,
    }
}

async fn import_artifact(
    conn: &mut SqliteConnection,
    artifact: &PackageArtifact,
) -> Result<(), CommitError> {
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT archive_sha256 FROM package_artifacts WHERE name = ? AND version = ?",
    )
    .bind(artifact.name.as_str())
    .bind(artifact.version.as_str())
    .fetch_optional(&mut *conn)
    .await
    .map_err(backend)?;
    if let Some((digest,)) = existing {
        let same = digest == artifact.archive_sha256.to_string();
        return Err(refuse(format!(
            "{}@{} is already imported{}",
            artifact.name,
            artifact.version,
            if same { "" } else { " with another digest" }
        )));
    }
    sqlx::query(
        "INSERT INTO package_artifacts (id, name, version, archive_sha256, key_id, archive_size, \
         signature_toml, signer_label, source, imported_by, imported_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(artifact.id.to_string())
    .bind(artifact.name.as_str())
    .bind(artifact.version.as_str())
    .bind(artifact.archive_sha256.to_string())
    .bind(artifact.key_id.as_str())
    .bind(int(artifact.archive_size))
    .bind(&artifact.signature_toml)
    .bind(artifact.signer_label.as_deref())
    .bind(artifact.source.as_str())
    .bind(artifact.imported_by.to_string())
    .bind(ms(artifact.imported_at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn delete_artifact(
    conn: &mut SqliteConnection,
    id: PackageArtifactId,
    expected: &ArtifactGuard,
) -> Result<(), CommitError> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT name, version, archive_sha256 FROM package_artifacts WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let Some((package, version, digest)) = row else {
        return Err(refuse("the artifact does not exist".to_owned()));
    };
    if digest != expected.archive_sha256.to_string() {
        return Err(refuse("the artifact changed since it was read".to_owned()));
    }
    let in_use: Option<(i64,)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT 1 FROM install_jobs j JOIN install_job_targets t ON t.job_id = j.id \
         WHERE j.kind = 'install' AND j.name = ? AND j.version = ? AND j.archive_sha256 = ? \
         AND t.status IN {ACTIVE} LIMIT 1"
    )))
    .bind(&package)
    .bind(&version)
    .bind(&digest)
    .fetch_optional(&mut *conn)
    .await
    .map_err(backend)?;
    if in_use.is_some() {
        return Err(refuse(format!(
            "{package}@{version} is used by a running install job"
        )));
    }
    sqlx::query("DELETE FROM package_artifacts WHERE id = ?")
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn create_job(
    conn: &mut SqliteConnection,
    job: &InstallJob,
    targets: &[InstallTarget],
) -> Result<(), CommitError> {
    if targets.is_empty() {
        return Err(refuse("a job needs at least one target".to_owned()));
    }
    if targets.iter().any(|target| target.job != job.id) {
        return Err(refuse("a target belongs to another job".to_owned()));
    }
    sqlx::query(
        "INSERT INTO install_jobs (id, kind, name, version, archive_sha256, key_id, \
         requested_by, created_at, job_plan_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(job.id.to_string())
    .bind(job.kind.as_str())
    .bind(job.name.as_str())
    .bind(job.version.as_str())
    .bind(job.archive_sha256.map(|d| d.to_string()))
    .bind(job.key_id.as_ref().map(|k| k.as_str().to_owned()))
    .bind(job.requested_by.to_string())
    .bind(ms(job.created_at))
    .bind(name(&job.job_plan_hash).map_err(map_store)?)
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    for target in targets {
        insert_target(conn, target).await?;
    }
    Ok(())
}

/// Inserts a target, refusing a host that already has a non-terminal one.
async fn insert_target(
    conn: &mut SqliteConnection,
    target: &InstallTarget,
) -> Result<(), CommitError> {
    if !target.status.is_terminal() {
        let busy: Option<(i64,)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT 1 FROM install_job_targets WHERE host_id = ? AND status IN {ACTIVE} LIMIT 1"
        )))
        .bind(target.host.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
        if busy.is_some() {
            return Err(refuse(format!(
                "host {} already has an install target in progress",
                target.host
            )));
        }
    }
    let b = TargetBinds::of(target).map_err(map_store)?;
    sqlx::query(
        "INSERT INTO install_job_targets (id, job_id, host_id, attempt, execution_id, status, \
         plan_hash, approved_by, approval_channel, approved_at, approval_session_ref, \
         approval_client, authorization_kind, expires_at, artifact_transfer, signature_transfer, \
         dispatched_session, dispatched_epoch, failure_code, failure_message, outputs, \
         outputs_confirmed, queued_at, started_at, finished_at, version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'human', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(b.id)
    .bind(b.job_id)
    .bind(b.host_id)
    .bind(b.attempt)
    .bind(b.execution_id)
    .bind(b.status)
    .bind(b.plan_hash)
    .bind(b.approved_by)
    .bind(b.approval_channel)
    .bind(b.approved_at)
    .bind(b.approval_session_ref)
    .bind(b.approval_client)
    .bind(b.expires_at)
    .bind(b.artifact_transfer)
    .bind(b.signature_transfer)
    .bind(b.dispatched_session)
    .bind(b.dispatched_epoch)
    .bind(b.failure_code)
    .bind(b.failure_message)
    .bind(b.outputs)
    .bind(b.outputs_confirmed)
    .bind(b.queued_at)
    .bind(b.started_at)
    .bind(b.finished_at)
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

fn conflict(job: InstallJobId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::install_job(job),
        expected,
        found,
    }
}

/// The stored status and version of a target.
async fn stored(
    conn: &mut SqliteConnection,
    id: InstallTargetId,
) -> Result<Option<(String, u64)>, CommitError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT status, version FROM install_job_targets WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    Ok(row.map(|(status, version)| (status, uint(version))))
}

/// Compares the stored target with the guard; `None` means it matches.
fn mismatch(
    job: InstallJobId,
    guard: TargetGuard,
    found: Option<(String, u64)>,
) -> Option<CommitError> {
    match found {
        None => Some(conflict(
            job,
            Expectation::Version(guard.version),
            Observed::Missing,
        )),
        Some((status, _)) if status != guard.status.as_str() => Some(conflict(
            job,
            Expectation::Status(guard.status.as_str().to_owned()),
            Observed::Status(status),
        )),
        Some((_, version)) if version != guard.version => Some(conflict(
            job,
            Expectation::Version(guard.version),
            Observed::Version(version),
        )),
        Some(_) => None,
    }
}

async fn update_target(
    conn: &mut SqliteConnection,
    target: &InstallTarget,
    guard: TargetGuard,
) -> Result<(), CommitError> {
    let found = stored(conn, target.id).await?;
    if let Some(error) = mismatch(target.job, guard, found) {
        return Err(error);
    }
    let b = TargetBinds::of(target).map_err(map_store)?;
    let done = sqlx::query(
        "UPDATE install_job_targets SET status = ?, artifact_transfer = ?, \
         signature_transfer = ?, dispatched_session = ?, dispatched_epoch = ?, \
         failure_code = ?, failure_message = ?, outputs = ?, outputs_confirmed = ?, \
         started_at = ?, finished_at = ?, version = version + 1 \
         WHERE id = ? AND status = ? AND version = ?",
    )
    .bind(b.status)
    .bind(b.artifact_transfer)
    .bind(b.signature_transfer)
    .bind(b.dispatched_session)
    .bind(b.dispatched_epoch)
    .bind(b.failure_code)
    .bind(b.failure_message)
    .bind(b.outputs)
    .bind(b.outputs_confirmed)
    .bind(b.started_at)
    .bind(b.finished_at)
    .bind(b.id)
    .bind(guard.status.as_str())
    .bind(int(guard.version))
    .execute(&mut *conn)
    .await
    .map_err(backend)?
    .rows_affected();
    if done == 0 {
        let found = stored(conn, target.id).await?;
        return Err(mismatch(target.job, guard, found).unwrap_or_else(|| {
            conflict(
                target.job,
                Expectation::Version(guard.version),
                Observed::Missing,
            )
        }));
    }
    Ok(())
}

async fn inventory(
    conn: &mut SqliteConnection,
    host: HostId,
    entries: &[HostPackage],
    replace: bool,
) -> Result<(), CommitError> {
    if entries.iter().any(|entry| entry.host != host) {
        return Err(refuse(
            "an inventory row belongs to another host".to_owned(),
        ));
    }
    for entry in entries {
        upsert_package(conn, entry).await?;
    }
    if replace {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        sqlx::query(
            "DELETE FROM host_packages WHERE host_id = ? \
             AND name NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(host.to_string())
        .bind(json(&names).map_err(map_store)?)
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

async fn upsert_package(
    conn: &mut SqliteConnection,
    entry: &HostPackage,
) -> Result<(), CommitError> {
    sqlx::query(
        "INSERT INTO host_packages (host_id, name, version, key_id, archive_sha256, installed_at, \
         status, key_state, skills, privilege_boundary, observed_at, observed_by) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (host_id, name) DO UPDATE SET version = excluded.version, \
         key_id = excluded.key_id, archive_sha256 = excluded.archive_sha256, \
         installed_at = excluded.installed_at, status = excluded.status, \
         key_state = excluded.key_state, skills = excluded.skills, \
         privilege_boundary = excluded.privilege_boundary, observed_at = excluded.observed_at, \
         observed_by = excluded.observed_by",
    )
    .bind(entry.host.to_string())
    .bind(entry.name.as_str())
    .bind(entry.version.as_str())
    .bind(entry.key_id.as_str())
    .bind(entry.archive_sha256.to_string())
    .bind(ms(entry.installed_at))
    .bind(name(&entry.status).map_err(map_store)?)
    .bind(name(&entry.key_state).map_err(map_store)?)
    .bind(json(&entry.skills).map_err(map_store)?)
    .bind(name(&entry.privilege_boundary).map_err(map_store)?)
    .bind(ms(entry.observed_at))
    .bind(entry.observed_by.as_str())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn remove_inventory(
    conn: &mut SqliteConnection,
    host: HostId,
    package: &PackageName,
) -> Result<(), CommitError> {
    sqlx::query("DELETE FROM host_packages WHERE host_id = ? AND name = ?")
        .bind(host.to_string())
        .bind(package.as_str())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}
