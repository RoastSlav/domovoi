//! Read access to what the hub holds about a host's helper.

use async_trait::async_trait;

use jiff::Timestamp;

use crate::attest::{LatestOutcome, Nonce32, TrustView, UnverifiedReason};
use crate::host::KeyFingerprint;
use crate::id::HostId;
use crate::ports::agent_link::{AttestationReply, LinkError};
use crate::ports::repos::StoreError;
use crate::ports::uow::HighWater;

/// Reads the pin, the live session's key and the latest attestation outcome of a host.
///
/// Implemented by the hub over the store and the live session. The attested coverage check and
/// the containment decision read through it and never write.
#[async_trait]
pub trait HelperTrustRead: Send + Sync {
    /// The trust view of `host`: its pin, the key its session reported, the latest outcome and
    /// the stored verified state. All fields are empty for a host never seen.
    async fn view(&self, host: HostId) -> Result<TrustView, StoreError>;
}

/// What the hub last recorded about a host's attestation, for display and audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOutcome {
    /// The latest outcome.
    pub outcome: LatestOutcome,
    /// The wire reason of a non-verified outcome.
    pub reason: Option<UnverifiedReason>,
    /// Internal detail of a non-verified outcome, for example `helper_incomplete:state_corrupt`.
    pub detail: Option<String>,
    /// When the hub recorded it.
    pub at: Timestamp,
}

/// What the store holds for a host: the pin, the latest outcome and the verified state, and
/// what the hub last recorded. `view.received_key` is always empty here; the live session
/// fills it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredTrust {
    /// The stored part of the trust view.
    pub view: TrustView,
    /// The latest recorded outcome, if any.
    pub last: Option<LastOutcome>,
}

/// Reads the stored helper trust rows. Writes go through the unit of work.
#[async_trait]
pub trait HelperTrustRepo: Send + Sync {
    /// The pin, the latest outcome and the verified state of `host`; empty for a host never
    /// seen.
    async fn stored(&self, host: HostId) -> Result<StoredTrust, StoreError>;

    /// The high-water row for `host` under the key with `fingerprint`.
    async fn high_water(
        &self,
        host: HostId,
        fingerprint: &KeyFingerprint,
    ) -> Result<Option<HighWater>, StoreError>;
}

/// What an agent's live session said about its helper. Unverified text: display only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperLive {
    /// No helper.
    Absent,
    /// The helper answered, with the operations it offers.
    Ok {
        /// The operation names it reported.
        ops: Vec<String>,
    },
    /// The helper exists but reported an error.
    Error {
        /// What the agent said.
        reason: String,
    },
}

/// The helper facts of one live session: what the agent said and the public key it relayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperReport {
    /// The helper's reported state.
    pub live: HelperLive,
    /// The 32 raw public-key bytes the agent relayed, if any. Never trusted: compared with
    /// the pin and shown for enrolment.
    pub public_key: Option<[u8; 32]>,
}

/// The narrow capability the helper trust code has over a host's agent link (ADR 0023): ask
/// for a signed report, read what the session says about its helper, and know whether the
/// host is connected. It holds nothing else the link can do.
#[async_trait]
pub trait HelperRelay: Send + Sync {
    /// Asks `host`'s helper, through its agent, for a signed safety report bound to `nonce`,
    /// waiting until `deadline`. The reply is unverified.
    async fn ask(
        &self,
        host: HostId,
        nonce: Nonce32,
        deadline: Timestamp,
    ) -> Result<AttestationReply, LinkError>;

    /// What `host`'s live session says about its helper; `None` without a live session.
    fn report(&self, host: HostId) -> Option<HelperReport>;

    /// Whether `host`'s agent is connected now.
    fn connected(&self, host: HostId) -> bool;
}
