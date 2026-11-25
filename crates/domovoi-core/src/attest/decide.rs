//! The pure decisions a containment request makes from the helper's verified state.
//!
//! These functions only decide; step-up, typed confirmation, permits and approval cards are
//! built on them elsewhere.

use jiff::Timestamp;

use crate::action::AuthorizationKind;

use super::expected::ExpectedSafety;
use super::gate::{covered, gate};
use super::identity::SafetyIdentity;
use super::reason::UnverifiedReason;
use super::scalar::Cidr;
use super::view::{TrustView, VerifiedState};

/// Who authorises the request at the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    /// An authenticated human approves it.
    Human,
    /// A policy permit authorises it, unattended.
    Policy,
}

/// What the hub decides for a containment request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The gate passed and every candidate is covered: dispatch with this identity.
    Verified(SafetyIdentity),
    /// Human authority only: proceed on the unverified path, showing these paths as
    /// unprotected. Step-up and typed confirmation are the caller's.
    UnverifiedHuman {
        /// Why coverage could not be verified.
        reason: UnverifiedReason,
        /// The candidate paths the verified report does not cover.
        unprotected: Vec<Cidr>,
    },
    /// Policy authority never proceeds on anything unverified.
    Refused(UnverifiedReason),
}

/// Decides a request for `candidates` under `authority`.
///
/// A failing gate gives its reason; a verified report that leaves a candidate uncovered gives
/// `path_uncovered`. Human authority then gets the unverified path, policy authority is
/// refused.
#[must_use]
pub fn decide(
    now: Timestamp,
    view: &TrustView,
    authority: Authority,
    candidates: &[Cidr],
) -> Decision {
    let checked = gate(now, view);
    let unprotected = unprotected_paths(checked, candidates);
    let reason = match checked {
        Err(reason) => reason,
        Ok(state) if unprotected.is_empty() => return Decision::Verified(state.identity.clone()),
        Ok(_) => UnverifiedReason::PathUncovered,
    };
    match authority {
        Authority::Human => Decision::UnverifiedHuman {
            reason,
            unprotected,
        },
        Authority::Policy => Decision::Refused(reason),
    }
}

/// The candidates the verified report does not cover; every candidate when the gate failed.
#[must_use]
pub fn unprotected_paths(
    view: Result<&VerifiedState, UnverifiedReason>,
    candidates: &[Cidr],
) -> Vec<Cidr> {
    match view {
        Err(_) => candidates.to_vec(),
        Ok(state) => candidates
            .iter()
            .filter(|cidr| !covered(state, cidr))
            .copied()
            .collect(),
    }
}

/// A request that pairs an unverified tag with policy authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an unverified helper requires a human approval")]
pub struct PolicyRequiresHuman;

/// The agent's own check before it calls the helper: policy authority never carries an
/// `Unverified` tag.
///
/// # Errors
///
/// [`PolicyRequiresHuman`] for `Unverified` with [`AuthorizationKind::Policy`].
pub fn agent_accepts(
    tag: &ExpectedSafety,
    authority: AuthorizationKind,
) -> Result<(), PolicyRequiresHuman> {
    match (tag, authority) {
        (ExpectedSafety::Unverified { .. }, AuthorizationKind::Policy) => Err(PolicyRequiresHuman),
        _ => Ok(()),
    }
}
