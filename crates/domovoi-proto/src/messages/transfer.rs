//! Package transfers: the hub sends bytes the agent stores.

use uuid::Uuid;

use crate::Open;
use crate::bytes::{Base64Bytes, Sha256Bytes};

/// Announce a transfer and the hash its bytes must have.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferBegin {
    /// The transfer this announces.
    pub transfer_id: Uuid,
    /// What is being sent.
    pub kind: Open<TransferKind>,
    /// The exact decoded size in bytes.
    pub size: u64,
    /// SHA-256 of the complete decoded bytes.
    pub sha256: Sha256Bytes,
}

/// A transfer kind this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    /// A package artifact.
    PackageArtifact,
    /// A package manifest.
    PackageManifest,
}

/// One chunk of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferChunk {
    /// The transfer this chunk belongs to.
    pub transfer_id: Uuid,
    /// Zero-based chunk sequence number.
    pub seq: u64,
    /// Base64 of at most 64 KiB of raw bytes.
    pub bytes: Base64Bytes,
}

/// Finish a transfer; the receiver verifies the hash and answers once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferEnd {
    /// The transfer this ends.
    pub transfer_id: Uuid,
}

/// The answer to `transfer_end`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferResult {
    /// The transfer this answers.
    pub transfer_id: Uuid,
    /// What happened.
    pub outcome: TransferOutcome,
}

/// Whether a transfer was stored.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferOutcome {
    /// The bytes are stored and the hash matched.
    Stored,
    /// The transfer failed.
    Failed {
        /// The failure class; an unknown value is preserved.
        code: Open<TransferFailureCode>,
        /// A short, redacted explanation.
        detail: String,
    },
}

/// A transfer failure class this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferFailureCode {
    /// The decoded bytes did not match the announced hash or size.
    ChecksumMismatch,
    /// The transfer exceeded its announced size.
    TooLarge,
    /// The bytes could not be stored.
    Storage,
    /// The sender aborted the transfer.
    Aborted,
}
