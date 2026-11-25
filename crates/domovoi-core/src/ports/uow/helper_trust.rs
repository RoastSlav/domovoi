//! The state writes of the helper trust aggregate, applied inside a
//! [`Transaction`](super::Transaction).

use jiff::Timestamp;

use crate::attest::{Hash32, LatestOutcome, SafetyIdentity, UnverifiedReason};
use crate::host::KeyFingerprint;
use crate::id::HostId;

/// The highest accepted identity of one host and key: the rollback and repeat guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighWater {
    /// The highest generation accepted.
    pub generation: u64,
    /// The config hash accepted at that generation.
    pub config_hash: Hash32,
    /// The route-state hash accepted at that generation.
    pub route_state_hash: Hash32,
}

/// A complete report that passed every verification step, ready to store.
///
/// Built only by the hub's acceptance module, from a value only the complete-report path of
/// the verifier produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The host.
    pub host: HostId,
    /// What the report proved.
    pub identity: SafetyIdentity,
    /// The report bytes as received.
    pub report: Vec<u8>,
    /// The hub's clock when it issued the nonce; freshness is measured from here.
    pub nonce_issued_at: Timestamp,
    /// When the hub accepted the report.
    pub accepted_at: Timestamp,
    /// The high-water row the verifier read. The write conflicts if the stored row moved.
    pub seen_high_water: Option<HighWater>,
}

/// A non-verified latest outcome to record. It also clears the stored verified state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeRecord {
    /// The host.
    pub host: HostId,
    /// The outcome; never [`LatestOutcome::Verified`].
    pub outcome: LatestOutcome,
    /// The wire reason the gate reports.
    pub reason: UnverifiedReason,
    /// Internal detail for audit and display, for example `helper_incomplete:state_corrupt`.
    pub detail: String,
    /// When the hub recorded it.
    pub at: Timestamp,
}

/// An owner's pin of a host's helper key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinWrite {
    /// The host.
    pub host: HostId,
    /// The 32 raw public-key bytes.
    pub public_key: [u8; 32],
    /// The fingerprint, computed by the hub from `public_key`.
    pub fingerprint: KeyFingerprint,
    /// When the owner pinned it.
    pub pinned_at: Timestamp,
    /// Who pinned it.
    pub pinned_by: String,
}

/// A write to the helper key pins, the latest attestation outcomes and the high-water marks.
#[derive(Debug, Clone)]
pub enum HelperTrustWrite {
    /// Stores a verified attestation, advances the high-water mark and sets the latest
    /// outcome to verified, all in this commit. The applier re-checks the high-water row and
    /// answers a conflict if it moved since the verifier read it.
    Accept(Accepted),
    /// Records a mismatch, a signed incomplete report or a regression and revokes the
    /// stored verified state.
    RecordOutcome(OutcomeRecord),
    /// Pins a key, deletes the high-water row of that host and key, and clears the latest
    /// outcome and verified state.
    Pin(PinWrite),
    /// Deletes the pin and the latest outcome. The high-water rows stay.
    Unpin {
        /// The host.
        host: HostId,
    },
    /// Deletes everything held for a host that is removed from the hub.
    RemoveHost {
        /// The host.
        host: HostId,
    },
}
