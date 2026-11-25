//! What the hub shows about a host's helper: every value is computed here from the pin, the
//! stored verified report and the clock. Text an agent reported is carried as such, apart.

use domovoi_core::HostId;
use domovoi_core::attest::{
    LatestOutcome, PinnedKey, Report, TrustView, UnverifiedReason, VerifiedState, gate,
    parse_report,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HelperReport, StoreError, StoredTrust};
use jiff::Timestamp;

use super::HelperTrustHub;

/// Where the host's helper key stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// The host reports no helper key and none is pinned.
    NoHelper,
    /// The hub holds a key from the helper that the owner has not pinned.
    NotPinned,
    /// The owner pinned the key and the helper presents the same one, or none.
    Pinned,
    /// The helper presents a key other than the pinned one.
    Changed,
}

/// The verdict on the attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationState {
    /// Verified and fresh.
    Verified,
    /// Verified, but older than the freshness window.
    Stale,
    /// Not verified.
    Unverified,
}

/// A key the helper's session relayed, with the fingerprint the hub computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedKey {
    /// The fingerprint of the relayed key, computed by the hub.
    pub fingerprint: KeyFingerprint,
    /// The first 16 hex characters of the fingerprint: a label.
    pub key_id: String,
}

/// Where the attestation stands and what the hub knows about why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationDetail {
    /// The verdict.
    pub state: AttestationState,
    /// The reason the gate refuses, when it does.
    pub reason: Option<UnverifiedReason>,
    /// The token of a validly signed incomplete report, display only.
    pub helper_reason: Option<String>,
    /// The unsigned token an agent relayed, an unverified hint.
    pub unsigned_hint: Option<String>,
    /// When the hub last recorded an outcome or asked.
    pub checked_at: Option<Timestamp>,
}

/// The verified effective set, shown only while the gate passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDetail {
    /// What the gate returned.
    pub state: VerifiedState,
    /// Seconds since the nonce was issued.
    pub age_seconds: i64,
    /// When the helper computed the report (display only; never trusted).
    pub computed_at: Timestamp,
    /// The protected unit names.
    pub protected_units: Vec<String>,
    /// The protected container names.
    pub protected_containers: Vec<String>,
}

/// Everything shown for one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperDetail {
    /// What the agent said about its helper, unverified.
    pub helper: Option<HelperReport>,
    /// The key the session relayed.
    pub received_key: Option<ReceivedKey>,
    /// The owner's pin.
    pub pin: Option<PinnedKey>,
    /// Where the key stands.
    pub key_state: KeyState,
    /// The attestation verdict.
    pub attestation: AttestationDetail,
    /// The verified set, only when the gate passes.
    pub verified: Option<VerifiedDetail>,
}

fn key_state(view: &TrustView) -> KeyState {
    match (&view.pin, view.received_key) {
        (None, None) => KeyState::NoHelper,
        (None, Some(_)) => KeyState::NotPinned,
        (Some(pin), Some(key)) if key != pin.public_key => KeyState::Changed,
        (Some(_), _) => KeyState::Pinned,
    }
}

fn helper_reason(stored: &StoredTrust) -> Option<String> {
    let last = stored.last.as_ref()?;
    if last.outcome != LatestOutcome::SignedIncomplete {
        return None;
    }
    last.detail
        .as_deref()?
        .strip_prefix("helper_incomplete:")
        .map(str::to_owned)
}

fn verified_detail(state: &VerifiedState, now: Timestamp) -> Option<VerifiedDetail> {
    let Ok(Report::Complete(report)) = parse_report(&state.report) else {
        return None;
    };
    let decoded = |lines: &[String]| {
        lines
            .iter()
            .map(|line| {
                domovoi_core::attest::decode_field(line).map_or_else(
                    |_| line.clone(),
                    |bytes| String::from_utf8_lossy(&bytes).into(),
                )
            })
            .collect()
    };
    Some(VerifiedDetail {
        age_seconds: now.duration_since(state.nonce_issued_at).as_secs(),
        computed_at: report.computed_at,
        protected_units: decoded(&report.config.protected_unit),
        protected_containers: decoded(&report.config.protected_container),
        state: state.clone(),
    })
}

impl HelperTrustHub {
    /// What to show for `host`'s helper now.
    ///
    /// # Errors
    /// A store failure.
    pub async fn detail(&self, host: HostId) -> Result<HelperDetail, StoreError> {
        let stored = self.stored_with_live(host).await?;
        let now = self.clock.now();
        let attempt = self.attempt_of(host);
        let helper = self.relay.report(host);
        let checked = gate(now, &stored.view);
        let (state, reason, verified) = match checked {
            Ok(state) => (
                AttestationState::Verified,
                None,
                verified_detail(state, now),
            ),
            Err(UnverifiedReason::AttestationStale) => (
                AttestationState::Stale,
                Some(UnverifiedReason::AttestationStale),
                None,
            ),
            Err(reason) => (AttestationState::Unverified, Some(reason), None),
        };
        Ok(HelperDetail {
            helper,
            received_key: stored.view.received_key.map(|key| {
                let fingerprint = KeyFingerprint::of_public_key(&key);
                ReceivedKey {
                    key_id: fingerprint.to_string().chars().take(16).collect(),
                    fingerprint,
                }
            }),
            key_state: key_state(&stored.view),
            attestation: AttestationDetail {
                state,
                reason,
                helper_reason: helper_reason(&stored),
                unsigned_hint: attempt.hint.clone(),
                checked_at: stored.last.as_ref().map(|last| last.at).or(attempt.at),
            },
            pin: stored.view.pin,
            verified,
        })
    }
}
