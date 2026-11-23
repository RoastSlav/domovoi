//! Hub side: nonce issue and single use, key pinning, the verification pipeline, acceptance,
//! generation regression and freshness (RW-37).

use std::collections::HashMap;

use crate::keys::{KeyError, b64_decode, b64_fixed, fingerprint, verify_report};
use crate::report::{EmptyReasonPolicy, ParseError, Report, parse_report};
use crate::safety::{SafetyIdentity, UnverifiedReason};

/// Nonce lifetime in seconds. A nonce is valid while `now - issued < NONCE_TTL` (the boundary is
/// not specified in the contract; this is the stricter reading).
pub const NONCE_TTL: u64 = 60;
/// Attestation freshness in seconds, measured from hub receipt. A report is fresh while
/// `now - accepted_at < FRESH_FOR` (reference clock and boundary are not specified).
pub const FRESH_FOR: u64 = 300;
/// Confirmation lifetime: 30 days.
pub const CONFIRMATION_MAX_AGE: u64 = 30 * 86_400;

/// Why a nonce was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NonceError {
    /// Never issued for this host (or already used).
    #[error("no outstanding nonce for this host")]
    Unknown,
    /// Outstanding for another host.
    #[error("nonce was issued for another host")]
    OtherHost,
    /// Older than 60 seconds.
    #[error("nonce expired")]
    Expired,
}

/// Outstanding nonces per host, single use.
#[derive(Debug, Default)]
pub struct NonceStore {
    issued: HashMap<[u8; 32], (String, u64)>,
}

impl NonceStore {
    /// Issue 32 random bytes for one host.
    pub fn issue(&mut self, host: &str, now: u64) -> [u8; 32] {
        let n = rand::random::<[u8; 32]>();
        self.issued.insert(n, (host.to_owned(), now));
        n
    }

    /// Consume a nonce for a host: removes it whatever the outcome once it is found.
    ///
    /// # Errors
    /// [`NonceError`].
    pub fn consume(&mut self, host: &str, nonce: &[u8; 32], now: u64) -> Result<(), NonceError> {
        match self.issued.get(nonce) {
            None => Err(NonceError::Unknown),
            Some((h, _)) if h != host => Err(NonceError::OtherHost),
            Some((_, at)) => {
                let age = now.saturating_sub(*at);
                self.issued.remove(nonce);
                if age < NONCE_TTL { Ok(()) } else { Err(NonceError::Expired) }
            }
        }
    }

    /// Number of nonces still outstanding.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.issued.len()
    }
}

/// How the hub sees a received key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    /// Not pinned: shown as unverified with this fingerprint.
    Unverified {
        /// Fingerprint the hub computed itself.
        fingerprint: String,
    },
    /// Equals the pin.
    Pinned,
    /// Differs from the pin.
    Mismatch {
        /// Fingerprint of the received key.
        received: String,
        /// Fingerprint of the kept pin.
        pinned: String,
    },
}

/// Why a pin request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinError {
    /// No step-up.
    #[error("step-up required")]
    StepUp,
    /// The owner's locally compared fingerprint does not match the received key.
    #[error("fingerprint does not match the received key")]
    FingerprintMismatch,
}

/// Pins and audit events per host.
#[derive(Debug, Default)]
pub struct Enrollment {
    pins: HashMap<String, [u8; 32]>,
    /// Audit event names in order (`helper.key_pinned`, `helper.key_rotated`, ...).
    pub audit: Vec<String>,
}

impl Enrollment {
    /// Classify a received key. Never pins.
    #[must_use]
    pub fn observe(&self, host: &str, key: &[u8; 32]) -> KeyStatus {
        match self.pins.get(host) {
            None => KeyStatus::Unverified { fingerprint: fingerprint(key) },
            Some(p) if p == key => KeyStatus::Pinned,
            Some(p) => KeyStatus::Mismatch { received: fingerprint(key), pinned: fingerprint(p) },
        }
    }

    /// The pinned key, if any.
    #[must_use]
    pub fn pinned(&self, host: &str) -> Option<[u8; 32]> {
        self.pins.get(host).copied()
    }

    /// Owner enrollment or re-enrollment: needs step-up and the fingerprint the owner read from
    /// `domovoi helper fingerprint` on the host. Pins the exact 32 bytes.
    ///
    /// # Errors
    /// [`PinError`].
    pub fn pin(&mut self, host: &str, key: &[u8; 32], typed_fingerprint: &str, step_up: bool) -> Result<(), PinError> {
        if !step_up {
            return Err(PinError::StepUp);
        }
        if typed_fingerprint != fingerprint(key) {
            return Err(PinError::FingerprintMismatch);
        }
        let ev = if self.pins.insert(host.to_owned(), *key).is_some() { "helper.key_rotated" } else { "helper.key_pinned" };
        self.audit.push(ev.to_owned());
        Ok(())
    }

    /// Owner unpin: no pin remains.
    pub fn unpin(&mut self, host: &str) {
        if self.pins.remove(host).is_some() {
            self.audit.push("helper.key_unpinned".to_owned());
        }
    }

    /// Host deleted from the hub: the pin goes with it.
    pub fn delete_host(&mut self, host: &str) {
        self.pins.remove(host);
    }

    /// Agent re-enrollment or upgrade: the helper pin is unchanged.
    pub fn agent_reenroll(&mut self, _host: &str) {}
}

/// The wire message `safety_attestation`.
#[derive(Debug, Clone)]
pub struct Attestation {
    /// Standard padded base64 of 32 bytes.
    pub public_key: String,
    /// Base64 of the report bytes.
    pub report: String,
    /// Base64 of 64 bytes.
    pub signature: String,
}

/// A report the hub accepted as the host's verified safety state.
#[derive(Debug, Clone)]
pub struct VerifiedState {
    /// Exact signed bytes.
    pub report_bytes: Vec<u8>,
    /// Parsed report.
    pub report: Report,
    /// The pinned key it verified under.
    pub public_key: [u8; 32],
    /// The identity.
    pub identity: SafetyIdentity,
    /// Hub receipt time (reference for freshness).
    pub accepted_at: u64,
}

/// Why verification failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    /// No pin.
    #[error("key not pinned")]
    KeyNotPinned,
    /// Key differs from the pin.
    #[error("key differs from the pinned key")]
    KeyMismatch,
    /// Public key, report or signature did not decode to the exact shape.
    #[error("bad encoding: {0}")]
    BadEncoding(KeyError),
    /// `verify_strict` failed.
    #[error("signature invalid")]
    SignatureInvalid,
    /// Signature fine, strict parse failed.
    #[error("report rejected: {0}")]
    Parse(ParseError),
    /// Nonce refused.
    #[error("nonce: {0}")]
    Nonce(NonceError),
    /// Lower generation than the highest verified under this key.
    #[error("generation_regressed: {got} < {highest}")]
    GenerationRegressed {
        /// Highest verified so far.
        highest: u64,
        /// Received.
        got: u64,
    },
    /// A complete report with generation 0 (init starts at 1).
    #[error("complete report with generation 0")]
    GenerationZero,
}

impl Rejection {
    /// Nearest `unverified` reason. The contract has no token for a validly signed but
    /// unparseable report or a bad nonce; the mapping here is a harness reading.
    #[must_use]
    pub fn unverified_reason(&self) -> UnverifiedReason {
        match self {
            Self::KeyNotPinned => UnverifiedReason::KeyNotPinned,
            Self::KeyMismatch => UnverifiedReason::KeyMismatch,
            Self::BadEncoding(_) | Self::SignatureInvalid => UnverifiedReason::SignatureInvalid,
            Self::Parse(_) | Self::GenerationZero => UnverifiedReason::ReportIncomplete,
            Self::Nonce(_) => UnverifiedReason::AttestationMissing,
            Self::GenerationRegressed { .. } => UnverifiedReason::GenerationRegressed,
        }
    }
}

/// Result of a verification that passed signature, parse and nonce.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Signature, nonce and `complete=true`: stored as verified safety state.
    Verified(Box<VerifiedState>),
    /// Signature and nonce passed but `complete=false`: not stored, not verified.
    SignedIncomplete {
        /// The reason token the helper gave.
        reason: Option<String>,
    },
}

/// The hub's attestation machinery for many hosts.
#[derive(Debug, Default)]
pub struct Hub {
    /// Pins.
    pub enroll: Enrollment,
    /// Nonces.
    pub nonces: NonceStore,
    highest: HashMap<[u8; 32], u64>,
    state: HashMap<String, VerifiedState>,
    /// Hosts whose helper presented a key other than the pin, with the pin it differed from.
    mismatch: HashMap<String, [u8; 32]>,
    /// Steps of the last `verify` call, in order (instrumentation for the ordering test).
    pub trace: Vec<&'static str>,
    /// The exact bytes handed to the strict parser in the last call.
    pub parsed_input: Option<Vec<u8>>,
    /// Parser policy for an empty `incomplete_reason`.
    pub policy: Option<EmptyReasonPolicy>,
}

impl Hub {
    /// Verify a received attestation: pinned key first, signature over the received bytes, then
    /// the strict parser on those same bytes, hash recomputation, nonce, regression, acceptance.
    ///
    /// # Errors
    /// [`Rejection`].
    pub fn verify(&mut self, host: &str, att: &Attestation, now: u64) -> Result<Outcome, Rejection> {
        self.trace.clear();
        self.parsed_input = None;
        self.trace.push("decode_public_key");
        let pk: [u8; 32] = b64_fixed(&att.public_key).map_err(Rejection::BadEncoding)?;
        self.trace.push("check_pin");
        match self.enroll.observe(host, &pk) {
            KeyStatus::Unverified { .. } => return Err(Rejection::KeyNotPinned),
            KeyStatus::Mismatch { .. } => {
                if let Some(p) = self.enroll.pinned(host) {
                    self.mismatch.insert(host.to_owned(), p);
                }
                return Err(Rejection::KeyMismatch);
            }
            KeyStatus::Pinned => {}
        }
        self.trace.push("decode_report_and_signature");
        let bytes = b64_decode(&att.report).map_err(Rejection::BadEncoding)?;
        let sig: [u8; 64] = b64_fixed(&att.signature).map_err(Rejection::BadEncoding)?;
        self.trace.push("verify_signature");
        if !verify_report(&pk, &bytes, &sig).map_err(Rejection::BadEncoding)? {
            return Err(Rejection::SignatureInvalid);
        }
        self.trace.push("parse_report");
        self.parsed_input = Some(bytes.clone());
        let policy = self.policy.unwrap_or(EmptyReasonPolicy::EmptyIffComplete);
        let report = parse_report(&bytes, policy).map_err(Rejection::Parse)?;
        self.trace.push("check_hashes");
        report.check_hashes().map_err(Rejection::Parse)?;
        self.trace.push("check_nonce");
        self.nonces.consume(host, &report.nonce, now).map_err(Rejection::Nonce)?;
        if !report.complete {
            return Ok(Outcome::SignedIncomplete { reason: report.incomplete_reason.clone() });
        }
        self.trace.push("check_generation");
        if report.generation == 0 {
            return Err(Rejection::GenerationZero);
        }
        let highest = self.highest.get(&pk).copied().unwrap_or(0);
        if report.generation < highest {
            return Err(Rejection::GenerationRegressed { highest, got: report.generation });
        }
        self.highest.insert(pk, report.generation);
        let identity = SafetyIdentity {
            fingerprint: fingerprint(&pk),
            boot_id: report.boot_id.clone(),
            generation: report.generation,
            config_hash: report.config_hash,
            route_state_hash: report.route_state_hash,
        };
        let vs = VerifiedState { report_bytes: bytes, report, public_key: pk, identity, accepted_at: now };
        self.state.insert(host.to_owned(), vs.clone());
        Ok(Outcome::Verified(Box::new(vs)))
    }

    /// The stored verified state for a host (only complete verified reports are ever stored).
    #[must_use]
    pub fn stored(&self, host: &str) -> Option<&VerifiedState> {
        self.state.get(host)
    }

    /// Highest verified generation under a key.
    #[must_use]
    pub fn highest_generation(&self, key: &[u8; 32]) -> Option<u64> {
        self.highest.get(key).copied()
    }

    /// The gate for the verified path: pinned key, verified complete attestation under 5
    /// minutes old. Returns the reason otherwise.
    ///
    /// # Errors
    /// The `unverified` reason.
    pub fn gate(&self, host: &str, now: u64) -> Result<&VerifiedState, UnverifiedReason> {
        let Some(pin) = self.enroll.pinned(host) else {
            return Err(UnverifiedReason::KeyNotPinned);
        };
        if self.mismatch.get(host) == Some(&pin) {
            return Err(UnverifiedReason::KeyMismatch);
        }
        let Some(vs) = self.state.get(host) else {
            return Err(UnverifiedReason::AttestationMissing);
        };
        if vs.public_key != pin {
            return Err(UnverifiedReason::KeyMismatch);
        }
        if now.saturating_sub(vs.accepted_at) >= FRESH_FOR {
            return Err(UnverifiedReason::AttestationStale);
        }
        Ok(vs)
    }
}
