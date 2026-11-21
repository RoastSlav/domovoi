//! Install jobs: the hub's record of putting signed packages on hosts and taking them off.
//!
//! Everything here is pure data and grammar. Importing files, relaying bytes and talking to
//! agents belong to the hub and the engine; this module fixes what the records mean.

mod artifact;
mod call;
mod failure;
mod inventory;
mod job;
mod status;
mod target;

pub use artifact::{ArtifactGuard, ArtifactSource, PackageArtifact};
pub use call::{
    CapabilityHandoff, MAX_CHUNK_BYTES, PackageCall, PackageDispatch, PackageOutputs,
    TransferAnswer, TransferFrame, TransferKind,
};
pub use failure::{MAX_FAILURE_MESSAGE_CHARS, REFUSAL_CODES, TargetFailure, codes};
pub use inventory::{HostPackage, ObservedBy};
pub use job::{InstallJob, InstallKind};
pub use status::{TargetGuard, TargetStatus};
pub use target::{InstallTarget, StoredAuthorization};

#[cfg(test)]
mod tests;
