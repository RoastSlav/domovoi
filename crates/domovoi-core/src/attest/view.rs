//! What the hub holds about one host's helper, as plain values the gate reads.

use core::net::IpAddr;

use jiff::Timestamp;

use crate::host::KeyFingerprint;

use super::identity::SafetyIdentity;
use super::report::CompleteReport;
use super::scalar::Cidr;

/// The helper key the owner pinned for a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedKey {
    /// The 32 raw public-key bytes. This is the only trust root: the key id is a label.
    pub public_key: [u8; 32],
    /// The fingerprint, computed from `public_key` by [`PinnedKey::new`].
    pub fingerprint: KeyFingerprint,
    /// When the owner pinned it.
    pub pinned_at: Timestamp,
    /// Who pinned it.
    pub pinned_by: String,
}

impl PinnedKey {
    /// A pin of `public_key`, with its fingerprint computed here.
    #[must_use]
    pub fn new(public_key: [u8; 32], pinned_at: Timestamp, pinned_by: impl Into<String>) -> Self {
        Self {
            fingerprint: KeyFingerprint::of_public_key(&public_key),
            public_key,
            pinned_at,
            pinned_by: pinned_by.into(),
        }
    }
}

/// The latest attestation outcome for a host. Trust follows this, not a sticky flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatestOutcome {
    /// The latest attestation verified.
    Verified,
    /// The helper presented a key other than the pinned one.
    Mismatch,
    /// The pinned key validly signed an incomplete report.
    SignedIncomplete,
    /// The generation regressed, or repeated with different hashes.
    Regressed,
}

impl LatestOutcome {
    /// The stable text stored in the database: `verified`, `mismatch`, `signed_incomplete` or
    /// `regressed`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Mismatch => "mismatch",
            Self::SignedIncomplete => "signed_incomplete",
            Self::Regressed => "regressed",
        }
    }

    /// Reads [`LatestOutcome::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "verified" => Some(Self::Verified),
            "mismatch" => Some(Self::Mismatch),
            "signed_incomplete" => Some(Self::SignedIncomplete),
            "regressed" => Some(Self::Regressed),
            _ => None,
        }
    }
}

/// A verified attestation, stored only when a complete report passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedState {
    /// What the attestation proved.
    pub identity: SafetyIdentity,
    /// The report bytes as received, kept so the verified set can be shown.
    pub report: Vec<u8>,
    /// The hub's own clock when it issued the nonce. Freshness is measured from here.
    pub nonce_issued_at: Timestamp,
    /// When the hub accepted the report. Display only.
    pub accepted_at: Timestamp,
    /// The longest a containment may last, in seconds.
    pub max_ttl_s: u64,
    /// How many containments may be active; zero allows none.
    pub max_active: u64,
    /// The configured management networks.
    pub cidrs: Vec<Cidr>,
    /// The hub addresses the helper found.
    pub hub_addrs: Vec<IpAddr>,
    /// The link peers the helper found.
    pub link_peers: Vec<(IpAddr, u16)>,
}

impl VerifiedState {
    /// The verified state a complete report proves. `report` is the received bytes the
    /// parsed `parsed` came from; `fingerprint` is the hub-computed fingerprint of the key that
    /// signed them.
    #[must_use]
    pub fn from_report(
        parsed: &CompleteReport,
        report: Vec<u8>,
        fingerprint: KeyFingerprint,
        nonce_issued_at: Timestamp,
        accepted_at: Timestamp,
    ) -> Self {
        Self {
            identity: SafetyIdentity {
                fingerprint,
                boot_id: parsed.boot_id.clone(),
                generation: parsed.generation,
                config_hash: parsed.config_hash,
                route_state_hash: parsed.route_state_hash,
            },
            report,
            nonce_issued_at,
            accepted_at,
            max_ttl_s: parsed.config.max_ttl_s,
            max_active: parsed.config.max_active,
            cidrs: parsed.cidrs(),
            hub_addrs: parsed.hub_addrs(),
            link_peers: parsed.link_peers(),
        }
    }
}

/// Everything the gate needs about one host's helper.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustView {
    /// The pinned key, if the owner pinned one.
    pub pin: Option<PinnedKey>,
    /// The public key the live session reported, if any. Never trusted: it is only compared
    /// with the pin.
    pub received_key: Option<[u8; 32]>,
    /// The latest attestation outcome, if any attestation was ever recorded.
    pub latest: Option<LatestOutcome>,
    /// The stored verified state, if the latest outcome left one.
    pub verified: Option<VerifiedState>,
}
