//! Pure hub coverage rule and confirmation store over verified attestation data (RW-37).

use crate::encoding::{Cidr, parse_cidr, parse_ip, parse_cidr_config};
use crate::hub::{CONFIRMATION_MAX_AGE, VerifiedState};
use crate::lines::LinkPeer;
use crate::safety::{SafetyIdentity, UnverifiedReason};

/// A candidate management path; its origin never influences coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Observed by the hub.
    HubObserved,
    /// Reported by the agent (untrusted; may only propose).
    AgentReported,
    /// Entered by the owner.
    Owner,
}

/// A candidate path.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The prefix.
    pub cidr: Cidr,
    /// Where it came from.
    pub source: Source,
}

/// Covered iff the candidate is a subset of a `cidr` line, or equals a `hub_addr` or
/// `link_peer` address. Uses the verified report only.
#[must_use]
pub fn covered(vs: &VerifiedState, cand: &Candidate) -> bool {
    let r = &vs.report;
    r.config.cidr.iter().filter_map(|c| parse_cidr(c).ok()).any(|c| cand.cidr.subset_of(&c))
        || r.routes.hub_addr.iter().filter_map(|a| parse_ip(a).ok()).any(|a| cand.cidr == Cidr::host(a))
        || r.routes.link_peer.iter().filter_map(|p| LinkPeer::parse(p).ok()).any(|p| cand.cidr == Cidr::host(p.ip))
}

/// Parse a candidate from owner text.
///
/// # Errors
/// Encoding error text.
pub fn candidate(text: &str, source: Source) -> Result<Candidate, String> {
    Ok(Candidate { cidr: parse_cidr_config(text).map_err(|e| e.to_string())?, source })
}

/// A stored owner confirmation: the path and the identity it was confirmed against.
#[derive(Debug, Clone)]
pub struct Confirmation {
    /// Confirmed path.
    pub path: Cidr,
    /// Identity at confirmation.
    pub identity: SafetyIdentity,
    /// When (unix seconds).
    pub confirmed_at: u64,
}

/// Why a confirmation could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmError {
    /// No verified fresh attestation.
    Unverifiable(UnverifiedReason),
    /// The verified attestation does not cover the path. The fix is local `helper.toml`
    /// configuration, never a hub edit.
    Uncovered,
}

/// Create a confirmation (the step-up and audit parts are product behaviour, not modelled).
///
/// # Errors
/// [`ConfirmError`].
pub fn confirm(
    gate: Result<&VerifiedState, UnverifiedReason>,
    cand: &Candidate,
    now: u64,
) -> Result<Confirmation, ConfirmError> {
    let vs = gate.map_err(ConfirmError::Unverifiable)?;
    if !covered(vs, cand) {
        return Err(ConfirmError::Uncovered);
    }
    Ok(Confirmation { path: cand.cidr, identity: vs.identity.clone(), confirmed_at: now })
}

/// Is a stored confirmation still current against the present verified state?
///
/// `boot_id` may differ only when fingerprint, generation and both hashes are equal and the
/// current attestation is fresh and verified (the reboot exception, confirmations only).
///
/// # Errors
/// The reason it is not current.
pub fn confirmation_current(
    c: &Confirmation,
    gate: Result<&VerifiedState, UnverifiedReason>,
    now: u64,
) -> Result<(), UnverifiedReason> {
    let vs = gate?;
    let (a, b) = (&c.identity, &vs.identity);
    let same = a.fingerprint == b.fingerprint
        && a.generation == b.generation
        && a.config_hash == b.config_hash
        && a.route_state_hash == b.route_state_hash;
    if !same || now.saturating_sub(c.confirmed_at) >= CONFIRMATION_MAX_AGE {
        return Err(UnverifiedReason::ConfirmationStale);
    }
    if !covered(vs, &Candidate { cidr: c.path, source: Source::Owner }) {
        return Err(UnverifiedReason::PathUncovered);
    }
    Ok(())
}
