//! Why a package command was refused, and why one target did not run.

use domovoi_core::install::codes;
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::LinkError;
use domovoi_core::{HostId, InstallJobId};

/// Why a command to the install service was refused. Nothing changed unless the variant says
/// otherwise.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PackageError {
    /// No archive is imported for that package and version.
    #[error("no package {name}@{version} is imported")]
    NoSuchPackage {
        /// The package.
        name: PackageName,
        /// The version.
        version: PackageVersion,
    },
    /// No job has that id.
    #[error("no such install job {0}")]
    NoSuchJob(InstallJobId),
    /// The job has no target on that host.
    #[error("the job has no target on host {0}")]
    NoSuchTarget(HostId),
    /// The host is not registered.
    #[error("unknown host {0}")]
    UnknownHost(HostId),
    /// The host has no usable enrolled agent key, so no plan can be bound to it.
    #[error("host {0} has no enrolled agent")]
    NotEnrolled(HostId),
    /// The same host was named twice.
    #[error("host {0} is named twice")]
    DuplicateHost(HostId),
    /// A job needs between 1 and 16 hosts.
    #[error("a job needs between 1 and 16 hosts")]
    HostCount,
    /// What the hub would do now differs from what the owner confirmed.
    #[error("the plan changed since it was shown")]
    PlanChanged,
    /// The host already has a job that has not finished.
    #[error("host {0} already has an install job in progress")]
    HostBusy(HostId),
    /// The request does not fit the target's state; the text says why.
    #[error("{0}")]
    Conflict(String),
    /// The host's agent could not be reached.
    #[error("{0}")]
    Link(LinkError),
    /// The agent's answer could not be read.
    #[error("the agent's answer could not be read: {0}")]
    BadAnswer(String),
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// The service is shutting down; trying again after a moment works.
    #[error("the install service is shutting down; try again")]
    Unavailable,
}

/// Why a target stopped before the agent was asked to act, or could not be handed over.
///
/// Each variant that proves nothing was installed carries a stored failure code; the worker
/// settles the target as not run with it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageExecError {
    /// The target carries no human approval.
    #[error("the target has no valid human authorization")]
    NotAuthorized,
    /// The target was authorized by a policy, which package jobs do not support.
    #[error("policy authorizations are not supported for packages")]
    NotYetSupported,
    /// The approval window closed before the work started.
    #[error("the approval expired before the work started")]
    Expired,
    /// The agent does not offer the package capability here; the text says why.
    #[error("{0}")]
    NotAdvertised(String),
    /// The agent speaks another protocol version.
    #[error("the agent speaks another protocol version")]
    AgentIncompatible,
    /// The host has no live agent session.
    #[error("the host has no connected agent")]
    HostUnavailable,
    /// The archive is not in the hub's store, or its index row is gone.
    #[error("{0}")]
    ArtifactMissing(String),
    /// The archive in the store is not the one the index describes.
    #[error("{0}")]
    ArtifactCorrupt(String),
    /// A transfer to the agent failed; the text is the agent's or the link's reason.
    #[error("{0}")]
    TransferFailed(String),
    /// The agent's session changed before the request was handed over.
    #[error("the agent session changed before the request was handed over")]
    SessionChanged,
    /// The request could not be handed to the agent.
    #[error("{0}")]
    HandoffFailed(String),
    /// Another writer changed the target; this run stops without settling it.
    #[error("the target changed meanwhile")]
    Superseded,
    /// The store failed; the target keeps its stored state and a later pass retries.
    #[error("store error: {0}")]
    Store(String),
}

impl PackageExecError {
    /// The stored failure code and message of the not-run settlement this error stands for,
    /// or `None` when the run must not settle the target (superseded, store failure).
    #[must_use]
    pub fn not_run(&self) -> Option<(&'static str, String)> {
        let code = match self {
            Self::NotAuthorized => "not_authorized",
            Self::NotYetSupported => "not_yet_supported",
            Self::Expired => codes::APPROVAL_EXPIRED,
            Self::NotAdvertised(_) => codes::NOT_ADVERTISED,
            Self::AgentIncompatible => codes::AGENT_INCOMPATIBLE,
            Self::HostUnavailable => codes::HOST_UNAVAILABLE,
            Self::ArtifactMissing(_) => codes::ARTIFACT_MISSING,
            Self::ArtifactCorrupt(_) => codes::ARTIFACT_CORRUPT,
            Self::TransferFailed(_) => codes::TRANSFER_FAILED,
            Self::SessionChanged => codes::SESSION_CHANGED,
            Self::HandoffFailed(_) => codes::HANDOFF_FAILED,
            Self::Superseded | Self::Store(_) => return None,
        };
        Some((code, self.to_string()))
    }
}
