//! The identity half of apply-time enforcement.
//!
//! Before the helper acts on a request that carries an `expected_safety` tag, it compares the
//! tag with the identity it computes now. Overlap, protected units, TTL, `max_active`, an empty
//! management list and the mutation limit are checks of the containment apply itself and live
//! with it; what is decided here is only whether the request was made against the safety set
//! the helper is enforcing at this moment.

use domovoi_core::attest::{ExpectedSafety, SafetyIdentity};

/// What the comparison did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    /// The tag was `Verified` and every one of its five fields equals the current identity.
    Compared,
    /// The tag was `Unverified`: the comparison is skipped, and nothing else about the request
    /// is relaxed.
    SkippedUnverified,
}

/// Why a request is refused by the guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardRefusal {
    /// The request carried no tag. Absence is never read as "no guard".
    MalformedSafety,
    /// The tag names a safety set that is not the one the helper enforces now.
    SafetyChanged,
    /// The helper cannot compute its current identity, so it cannot compare.
    SafetyUnknown,
}

/// The current identity could not be computed, with the token that says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeFailure {
    /// The incomplete-report token or unsigned token of the failure.
    pub token: String,
}

/// Checks the request's tag against the identity computed now.
///
/// Order: an absent tag is [`GuardRefusal::MalformedSafety`]; an uncomputable current identity
/// is [`GuardRefusal::SafetyUnknown`]; a `Verified` tag is compared on all five fields (the
/// helper's own key fingerprint, boot id, generation and both hashes) and any difference is
/// [`GuardRefusal::SafetyChanged`]; an `Unverified` tag skips only the comparison.
///
/// # Errors
///
/// A [`GuardRefusal`] as above.
pub fn check_expected(
    expected: Option<&ExpectedSafety>,
    current: Result<&SafetyIdentity, ComputeFailure>,
) -> Result<GuardMode, GuardRefusal> {
    let expected = domovoi_core::attest::require_expected_safety(expected)
        .map_err(|_| GuardRefusal::MalformedSafety)?;
    let current = current.map_err(|_| GuardRefusal::SafetyUnknown)?;
    match expected.identity() {
        Some(wanted) if wanted == *current => Ok(GuardMode::Compared),
        Some(_) => Err(GuardRefusal::SafetyChanged),
        None => Ok(GuardMode::SkippedUnverified),
    }
}
