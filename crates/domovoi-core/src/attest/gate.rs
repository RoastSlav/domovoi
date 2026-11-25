//! The latest-outcome gate and its pure companions.
//!
//! The data types live in a sibling module and are re-exported here, so `attest::gate` is the
//! one place a caller finds the gate and what it reads.

use jiff::Timestamp;

use super::identity::{IdentityMatch, SafetyIdentity, compare};
use super::reason::UnverifiedReason;
use super::scalar::Cidr;
use super::{CONFIRMATION_MAX_AGE, FRESHNESS};

pub use super::view::{LatestOutcome, PinnedKey, TrustView, VerifiedState};

/// Whether the host's helper is verified and fresh right now.
///
/// The checks run in order: no pin is `key_not_pinned`; a received key that differs from the
/// pin, or a latest outcome of mismatch, is `key_mismatch`; a signed incomplete report is
/// `report_incomplete`; a regression is `generation_regressed`; no stored verified state is
/// `attestation_missing`; and a state whose nonce was issued [`FRESHNESS`] or more ago is
/// `attestation_stale`.
///
/// # Errors
///
/// The [`UnverifiedReason`] of the first check that fails.
pub fn gate(now: Timestamp, view: &TrustView) -> Result<&VerifiedState, UnverifiedReason> {
    let pin = view.pin.as_ref().ok_or(UnverifiedReason::KeyNotPinned)?;
    let key_differs = view.received_key.is_some_and(|key| key != pin.public_key);
    if key_differs || view.latest == Some(LatestOutcome::Mismatch) {
        return Err(UnverifiedReason::KeyMismatch);
    }
    match view.latest {
        Some(LatestOutcome::SignedIncomplete) => return Err(UnverifiedReason::ReportIncomplete),
        Some(LatestOutcome::Regressed) => return Err(UnverifiedReason::GenerationRegressed),
        _ => {}
    }
    let state = view
        .verified
        .as_ref()
        .ok_or(UnverifiedReason::AttestationMissing)?;
    if is_fresh(now, state) {
        Ok(state)
    } else {
        Err(UnverifiedReason::AttestationStale)
    }
}

fn is_fresh(now: Timestamp, state: &VerifiedState) -> bool {
    now.duration_since(state.nonce_issued_at) < FRESHNESS
}

/// Whether the verified report covers `cidr`: it lies inside one of the configured networks,
/// or is exactly the host prefix of a hub address or link peer. Data an agent reported never
/// counts.
#[must_use]
pub fn covered(state: &VerifiedState, cidr: &Cidr) -> bool {
    state.cidrs.iter().any(|outer| cidr.subset_of(outer))
        || state
            .hub_addrs
            .iter()
            .any(|addr| *cidr == Cidr::host(*addr))
        || state
            .link_peers
            .iter()
            .any(|(addr, _)| *cidr == Cidr::host(*addr))
}

/// Whether an owner's stored confirmation is still current against the verified state.
///
/// The identity must be exactly equal, or differ only in the boot id while the current state
/// is fresh (a reboot does not invalidate a confirmation by itself); and the confirmation must
/// be younger than [`CONFIRMATION_MAX_AGE`].
///
/// # Errors
///
/// [`UnverifiedReason::ConfirmationStale`] otherwise.
pub fn confirmation_current(
    stored: &SafetyIdentity,
    confirmed_at: Timestamp,
    current: &VerifiedState,
    now: Timestamp,
) -> Result<(), UnverifiedReason> {
    let stale = UnverifiedReason::ConfirmationStale;
    if now.duration_since(confirmed_at) >= CONFIRMATION_MAX_AGE {
        return Err(stale);
    }
    match compare(stored, &current.identity) {
        IdentityMatch::Exact => Ok(()),
        IdentityMatch::BootOnlyDiffers if is_fresh(now, current) => Ok(()),
        IdentityMatch::BootOnlyDiffers | IdentityMatch::Different => Err(stale),
    }
}

/// Whether a permit's stored identity still equals the current one. All five fields must be
/// equal, the boot id included: the reboot exception is for confirmations only.
#[must_use]
pub fn permit_identity_current(stored: &SafetyIdentity, current: &SafetyIdentity) -> bool {
    stored == current
}
