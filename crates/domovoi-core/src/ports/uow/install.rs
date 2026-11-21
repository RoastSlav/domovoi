//! The state writes of the install aggregates, applied inside a
//! [`Transaction`](super::Transaction).

use crate::id::{HostId, PackageArtifactId};
use crate::install::{
    ArtifactGuard, HostPackage, InstallJob, InstallTarget, PackageArtifact, TargetGuard,
};
use crate::package::PackageName;

/// A write to the package artifacts, install jobs and host inventory. Built only by the
/// install service and the hub's package import.
#[derive(Debug, Clone)]
pub enum InstallWrite {
    /// Adds an imported archive to the index. A second row for the same name and version
    /// with another digest is [`CommitError::Constraint`](super::CommitError::Constraint).
    ImportArtifact(PackageArtifact),
    /// Removes an artifact row. Refused while a non-terminal target references it.
    DeleteArtifact {
        /// The artifact.
        id: PackageArtifactId,
        /// What the stored row must still look like.
        expected: ArtifactGuard,
    },
    /// Creates a job and all its first-attempt targets in one step. A host that already has
    /// a non-terminal target fails the whole write.
    CreateJob {
        /// The job.
        job: InstallJob,
        /// Its targets, one per host.
        targets: Vec<InstallTarget>,
    },
    /// Adds a retry attempt for a host of an existing job. The host must have no non-terminal
    /// target.
    NewAttempt(InstallTarget),
    /// Replaces a target whose stored status and version are as guarded; a mismatch is a
    /// conflict. The store bumps `version`.
    Target {
        /// The target to store.
        target: InstallTarget,
        /// What the stored target must look like.
        expected: TargetGuard,
    },
    /// Upserts a host's installed packages. With `replace`, rows for that host that are not
    /// in `entries` are deleted in the same step.
    Inventory {
        /// The host.
        host: HostId,
        /// The rows to store.
        entries: Vec<HostPackage>,
        /// Whether `entries` is the host's complete list.
        replace: bool,
    },
    /// Deletes one package's row from a host's inventory, after an uninstall.
    RemoveInventory {
        /// The host.
        host: HostId,
        /// The package.
        name: PackageName,
    },
}
