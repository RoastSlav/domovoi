//! Safety identity, the tagged `expected_safety` value and its strict parser (RW-37).

use serde_json::{Value, json};

use crate::hash::parse_hex32;

/// `(fingerprint, boot_id, generation, config_hash, route_state_hash)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyIdentity {
    /// Lowercase hex SHA-256 of the pinned public key.
    pub fingerprint: String,
    /// Boot id.
    pub boot_id: String,
    /// Generation.
    pub generation: u64,
    /// Config hash.
    pub config_hash: [u8; 32],
    /// Route state hash.
    pub route_state_hash: [u8; 32],
}

/// The nine `unverified` reasons of RW-37.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnverifiedReason {
    /// No pinned key.
    KeyNotPinned,
    /// Received key differs from the pin.
    KeyMismatch,
    /// No attestation held.
    AttestationMissing,
    /// Attestation older than 5 minutes.
    AttestationStale,
    /// Signature (or its encoding) invalid.
    SignatureInvalid,
    /// Report incomplete or unusable.
    ReportIncomplete,
    /// Generation went backwards.
    GenerationRegressed,
    /// A path is not covered.
    PathUncovered,
    /// A confirmation is stale.
    ConfirmationStale,
}

impl UnverifiedReason {
    /// All reasons, in the RW-37 order.
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
    /// Wire token.
    #[must_use]
    pub fn token(self) -> &'static str {
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
    /// Parse a wire token.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.token() == s)
    }
}

/// The tagged expected-safety value. `Unverified` carries no identity by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedSafety {
    /// A full identity.
    Verified(SafetyIdentity),
    /// No identity, only a reason.
    Unverified(UnverifiedReason),
}

/// Refusal of a malformed `expected_safety` (`refused{malformed_safety}`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed_safety: {0}")]
pub struct MalformedSafety(pub String);

fn m(s: &str) -> MalformedSafety {
    MalformedSafety(s.to_owned())
}

impl ExpectedSafety {
    /// JSON form: tag key `state` (the tag key name is a harness reading; the contract does not
    /// name it).
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Verified(i) => json!({
                "state": "verified",
                "fingerprint": i.fingerprint,
                "boot_id": i.boot_id,
                "generation": i.generation,
                "config_hash": hex::encode(i.config_hash),
                "route_state_hash": hex::encode(i.route_state_hash),
            }),
            Self::Unverified(r) => json!({"state": "unverified", "reason": r.token()}),
        }
    }

    /// Strict parse: exact key set per tag, no extra or missing field, no unknown tag or reason.
    ///
    /// # Errors
    /// [`MalformedSafety`] for any deviation.
    pub fn parse(v: &Value) -> Result<Self, MalformedSafety> {
        let o = v.as_object().ok_or_else(|| m("not an object"))?;
        let tag = o.get("state").and_then(Value::as_str).ok_or_else(|| m("missing tag"))?;
        let exact = |keys: &[&str]| -> Result<(), MalformedSafety> {
            if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
                return Err(m("field set differs from the tag's definition"));
            }
            Ok(())
        };
        let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or_else(|| m("field type"));
        match tag {
            "verified" => {
                exact(&["state", "fingerprint", "boot_id", "generation", "config_hash", "route_state_hash"])?;
                let fp = s("fingerprint")?;
                if parse_hex32(fp).is_none() {
                    return Err(m("fingerprint"));
                }
                Ok(Self::Verified(SafetyIdentity {
                    fingerprint: fp.to_owned(),
                    boot_id: s("boot_id")?.to_owned(),
                    generation: o.get("generation").and_then(Value::as_u64).ok_or_else(|| m("generation"))?,
                    config_hash: parse_hex32(s("config_hash")?).ok_or_else(|| m("config_hash"))?,
                    route_state_hash: parse_hex32(s("route_state_hash")?).ok_or_else(|| m("route_state_hash"))?,
                }))
            }
            "unverified" => {
                exact(&["state", "reason"])?;
                UnverifiedReason::parse(s("reason")?).map(Self::Unverified).ok_or_else(|| m("unknown reason"))
            }
            _ => Err(m("unknown tag")),
        }
    }
}
