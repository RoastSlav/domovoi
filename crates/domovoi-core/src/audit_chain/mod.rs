//! The audit hash chain: canonical encoding, the anchor over legacy history and the verifier.
//!
//! New audit rows carry `prev_hash` and `hash` columns. The first chained row links to an
//! [`AuditAnchor`] that commits to every row written before the chain existed. Hashing lives in
//! this crate so the encoding is defined once; the store computes it inside the transaction that
//! inserts the row.

mod encode;
mod verify;

pub use encode::{AuditAnchor, LegacyDigest, canonical_json, entry_hash};
pub use verify::{
    BreakReason, ChainBreak, ChainStart, ChainStatus, ChainVerifier, LegacyStatus, VerifyReport,
    VerifyScope,
};

use crate::ports::repos::AuditEvent;

/// A SHA-256 digest.
pub type Hash = [u8; 32];

/// The kind of a checkpoint row a pruner writes before deleting old audit rows.
pub const CHECKPOINT_KIND: &str = "audit.checkpoint";

/// One stored audit row with its chain fields. Both are `None` on a legacy row.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainRow {
    /// The row's sequence number.
    pub seq: u64,
    /// The hash of the row before it (or the anchor), when the row is chained.
    pub prev_hash: Option<Hash>,
    /// This row's own hash, when the row is chained.
    pub hash: Option<Hash>,
    /// The event as stored.
    pub event: AuditEvent,
}
