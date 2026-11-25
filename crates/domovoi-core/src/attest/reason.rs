//! The closed set of wire reasons a helper is unverified, and the open token of an incomplete
//! report.

use serde::{Deserialize, Serialize};

use crate::knowledge::UnverifiableReason;

/// Why text is not an [`UnverifiedReason`] token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown unverified reason {0:?}")]
pub struct UnverifiedReasonError(pub String);

/// Why the helper's safety set is unverified. Closed: it travels on the wire in
/// `expected_safety`, and a value this build does not know is refused rather than preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnverifiedReason {
    /// No key is pinned for the host's helper.
    KeyNotPinned,
    /// The helper presented a different key than the pinned one.
    KeyMismatch,
    /// No attestation is held.
    AttestationMissing,
    /// The held attestation is older than the freshness window.
    AttestationStale,
    /// The signature, or its encoding, is invalid.
    SignatureInvalid,
    /// The report is incomplete or unusable.
    ReportIncomplete,
    /// The generation went backwards or repeated with different hashes.
    GenerationRegressed,
    /// A path is not covered by the verified report.
    PathUncovered,
    /// A confirmation no longer matches the verified report.
    ConfirmationStale,
}

impl UnverifiedReason {
    /// Every reason, in wire order.
    pub const ALL: [Self; 9] = [
        Self::KeyNotPinned,
        Self::KeyMismatch,
        Self::AttestationMissing,
        Self::AttestationStale,
        Self::SignatureInvalid,
        Self::ReportIncomplete,
        Self::GenerationRegressed,
        Self::PathUncovered,
        Self::ConfirmationStale,
    ];

    /// The wire token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::KeyNotPinned => "key_not_pinned",
            Self::KeyMismatch => "key_mismatch",
            Self::AttestationMissing => "attestation_missing",
            Self::AttestationStale => "attestation_stale",
            Self::SignatureInvalid => "signature_invalid",
            Self::ReportIncomplete => "report_incomplete",
            Self::GenerationRegressed => "generation_regressed",
            Self::PathUncovered => "path_uncovered",
            Self::ConfirmationStale => "confirmation_stale",
        }
    }

    /// Parses a wire token.
    ///
    /// # Errors
    ///
    /// [`UnverifiedReasonError`] for any text that is not one of the nine tokens.
    pub fn parse(text: &str) -> Result<Self, UnverifiedReasonError> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.token() == text)
            .ok_or_else(|| UnverifiedReasonError(text.to_owned()))
    }
}

impl From<UnverifiedReason> for UnverifiableReason {
    fn from(reason: UnverifiedReason) -> Self {
        match reason {
            UnverifiedReason::KeyNotPinned => Self::KeyNotPinned,
            UnverifiedReason::KeyMismatch => Self::KeyMismatch,
            UnverifiedReason::AttestationMissing => Self::AttestationMissing,
            UnverifiedReason::AttestationStale => Self::AttestationStale,
            UnverifiedReason::SignatureInvalid => Self::SignatureInvalid,
            UnverifiedReason::ReportIncomplete => Self::ReportIncomplete,
            UnverifiedReason::GenerationRegressed => Self::GenerationRegressed,
            UnverifiedReason::PathUncovered => Self::PathUncovered,
            UnverifiedReason::ConfirmationStale => Self::ConfirmationStale,
        }
    }
}

impl From<UnverifiableReason> for UnverifiedReason {
    /// The wire reason for a coverage failure. `NotAvailable` is internal and goes out as
    /// `attestation_missing`.
    fn from(reason: UnverifiableReason) -> Self {
        match reason {
            UnverifiableReason::NotAvailable | UnverifiableReason::AttestationMissing => {
                Self::AttestationMissing
            }
            UnverifiableReason::KeyNotPinned => Self::KeyNotPinned,
            UnverifiableReason::KeyMismatch => Self::KeyMismatch,
            UnverifiableReason::AttestationStale => Self::AttestationStale,
            UnverifiableReason::SignatureInvalid => Self::SignatureInvalid,
            UnverifiableReason::ReportIncomplete => Self::ReportIncomplete,
            UnverifiableReason::GenerationRegressed => Self::GenerationRegressed,
            UnverifiableReason::PathUncovered => Self::PathUncovered,
            UnverifiableReason::ConfirmationStale => Self::ConfirmationStale,
        }
    }
}

/// The token of an incomplete report: `[a-z][a-z0-9_]{0,63}`.
///
/// The set a helper signs is the table of known tokens below; the grammar admits any token so
/// a hub shows one it does not know instead of refusing the report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IncompleteReason(String);

impl IncompleteReason {
    /// The key is present and `state` is absent.
    pub const STATE_MISSING: &'static str = "state_missing";
    /// The state's checksum, parse or format failed.
    pub const STATE_CORRUPT: &'static str = "state_corrupt";
    /// The state's key fingerprint is not the key's.
    pub const STATE_KEY_MISMATCH: &'static str = "state_key_mismatch";
    /// `helper.toml` parses but a value fails its grammar.
    pub const CONFIG_INVALID: &'static str = "config_invalid";
    /// `helper.toml` or `agent.toml` cannot be read or parsed as TOML.
    pub const CONFIG_UNREADABLE: &'static str = "config_unreadable";
    /// The policy rules could not be read.
    pub const RULES_UNREADABLE: &'static str = "rules_unreadable";
    /// A referenced routing table could not be read.
    pub const TABLE_UNREADABLE: &'static str = "table_unreadable";
    /// The interface addresses could not be read.
    pub const ADDRESSES_UNREADABLE: &'static str = "addresses_unreadable";
    /// A route lookup failed.
    pub const LOOKUP_FAILED: &'static str = "lookup_failed";
    /// A read returned output that could not be completed.
    pub const ROUTE_READ_PARTIAL: &'static str = "route_read_partial";
    /// A rule or route the report cannot express.
    pub const ROUTE_UNREPRESENTABLE: &'static str = "route_unrepresentable";
    /// The new state could not be made durable.
    pub const STATE_WRITE_FAILED: &'static str = "state_write_failed";
    /// The complete report would exceed the size cap.
    pub const REPORT_TOO_LARGE: &'static str = "report_too_large";

    /// Every token a helper signs.
    pub const KNOWN: [&'static str; 13] = [
        Self::STATE_MISSING,
        Self::STATE_CORRUPT,
        Self::STATE_KEY_MISMATCH,
        Self::CONFIG_INVALID,
        Self::CONFIG_UNREADABLE,
        Self::RULES_UNREADABLE,
        Self::TABLE_UNREADABLE,
        Self::ADDRESSES_UNREADABLE,
        Self::LOOKUP_FAILED,
        Self::ROUTE_READ_PARTIAL,
        Self::ROUTE_UNREPRESENTABLE,
        Self::STATE_WRITE_FAILED,
        Self::REPORT_TOO_LARGE,
    ];

    /// Validates the token grammar.
    #[must_use]
    pub fn new(token: &str) -> Option<Self> {
        let bytes = token.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.len() <= 64
            && bytes[0].is_ascii_lowercase()
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
        valid.then(|| Self(token.to_owned()))
    }

    /// The token text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the token is one a helper signs.
    #[must_use]
    pub fn is_known(&self) -> bool {
        Self::KNOWN.contains(&self.0.as_str())
    }
}
