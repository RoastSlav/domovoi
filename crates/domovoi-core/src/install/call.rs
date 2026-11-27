//! What the hub sends an agent for a package job: transfer frames and one closed capability
//! call. These are the value types of the link; the wire encoding lives in the adapter.

use std::collections::BTreeMap;

use jiff::Timestamp;
use uuid::Uuid;

use crate::action::PlanHash;
use crate::id::{ExecutionId, RequestId};
use crate::package::{PackageName, PackageVersion, keys};

/// The most bytes one chunk frame carries.
pub const MAX_CHUNK_BYTES: usize = 65_536;

/// What a transfer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// A package archive.
    PackageArtifact,
    /// A package's detached signature file.
    PackageManifest,
}

/// One frame of a transfer. A transfer is a `Begin`, chunks numbered from 0 without gaps, and
/// an `End`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferFrame {
    /// Announces the transfer and the hash its bytes must have.
    Begin {
        /// The transfer id.
        transfer: Uuid,
        /// What is being sent.
        kind: TransferKind,
        /// The exact size in bytes.
        size: u64,
        /// SHA-256 of all the bytes.
        sha256: [u8; 32],
    },
    /// One chunk of 1 to [`MAX_CHUNK_BYTES`] bytes.
    Chunk {
        /// The transfer id.
        transfer: Uuid,
        /// Zero-based position.
        seq: u64,
        /// The raw bytes.
        bytes: Vec<u8>,
    },
    /// Finishes the transfer; the agent verifies and answers once.
    End {
        /// The transfer id.
        transfer: Uuid,
    },
}

impl TransferFrame {
    /// The transfer this frame belongs to.
    #[must_use]
    pub fn transfer(&self) -> Uuid {
        match self {
            Self::Begin { transfer, .. }
            | Self::Chunk { transfer, .. }
            | Self::End { transfer } => *transfer,
        }
    }
}

/// The agent's answer to an `End` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferAnswer {
    /// The bytes are stored and the hash matched.
    Stored,
    /// The agent could not store them.
    Failed {
        /// The failure class.
        code: String,
        /// A short explanation from the agent.
        detail: String,
    },
}

/// The only calls the hub makes to the package capabilities. The set is closed and has no
/// key argument: which keys a host trusts is the host's own business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCall {
    /// Verify and install a package whose files arrived in two transfers.
    Install {
        /// The package name.
        package: PackageName,
        /// The package version.
        version: PackageVersion,
        /// The transfer that carried the archive.
        artifact_transfer: Uuid,
        /// The transfer that carried the signature file.
        signature_transfer: Uuid,
    },
    /// Remove an installed package.
    Uninstall {
        /// The package name.
        package: PackageName,
        /// The version to remove.
        version: PackageVersion,
    },
    /// Re-verify a stored package's files and let its skills run.
    Enable {
        /// The package name.
        package: PackageName,
        /// The stored version.
        version: PackageVersion,
    },
    /// Stop a stored package's skills from running; the files stay.
    Disable {
        /// The package name.
        package: PackageName,
        /// The stored version.
        version: PackageVersion,
    },
    /// List what is installed.
    List,
}

impl PackageCall {
    /// The capability contract this call runs, version 1.
    #[must_use]
    pub const fn contract_id(&self) -> &'static str {
        match self {
            Self::Install { .. } => "agent.package.install",
            Self::Uninstall { .. } => "agent.package.uninstall",
            Self::Enable { .. } => "agent.package.enable",
            Self::Disable { .. } => "agent.package.disable",
            Self::List => "agent.package.list",
        }
    }

    /// The capability arguments, keyed only by the `ARG_*` constants of the package contract.
    #[must_use]
    pub fn args(&self) -> BTreeMap<&'static str, String> {
        let mut args = BTreeMap::new();
        match self {
            Self::Install {
                package,
                version,
                artifact_transfer,
                signature_transfer,
            } => {
                args.insert(keys::ARG_PACKAGE, package.to_string());
                args.insert(keys::ARG_VERSION, version.to_string());
                args.insert(keys::ARG_ARTIFACT_TRANSFER, artifact_transfer.to_string());
                args.insert(keys::ARG_SIGNATURE_TRANSFER, signature_transfer.to_string());
            }
            Self::Uninstall { package, version }
            | Self::Enable { package, version }
            | Self::Disable { package, version } => {
                args.insert(keys::ARG_PACKAGE, package.to_string());
                args.insert(keys::ARG_VERSION, version.to_string());
            }
            Self::List => {}
        }
        args
    }

    /// The target text of the request: `package:<name>`, or `packages:all` for a list.
    #[must_use]
    pub fn target_text(&self) -> String {
        match self {
            Self::Install { package, .. }
            | Self::Uninstall { package, .. }
            | Self::Enable { package, .. }
            | Self::Disable { package, .. } => {
                format!("package:{package}")
            }
            Self::List => "packages:all".to_owned(),
        }
    }
}

/// One package capability request, ready to hand to a host's live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDispatch {
    /// The idempotency key of this execution.
    pub execution: ExecutionId,
    /// What to run.
    pub call: PackageCall,
    /// The plan hash the owner approved.
    pub plan_hash: PlanHash,
    /// When the request stops being valid.
    pub expires_at: Timestamp,
}

/// A capability request the hub has handed to an agent's session.
///
/// It exists from the moment the encoded frame is in that session's outgoing queue, after
/// which the agent may have started. `PackageRelay::package_result` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityHandoff {
    request: RequestId,
}

impl CapabilityHandoff {
    /// Wraps the id of the request frame the hub queued.
    #[must_use]
    pub const fn new(request: RequestId) -> Self {
        Self { request }
    }

    /// The id the queued frame was tagged with.
    #[must_use]
    pub const fn request(self) -> RequestId {
        self.request
    }
}

/// What a package capability reported on success: its outputs, keyed by the `OUT_*` constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOutputs(pub BTreeMap<String, String>);
