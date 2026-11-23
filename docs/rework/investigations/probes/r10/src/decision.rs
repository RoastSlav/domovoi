//! Simulated hub and agent decision functions for `Contain` (RW-37). The authenticated surface
//! (step-up, typed confirmation prompts, audit writing) is product behaviour and only modelled
//! as booleans here.

use serde_json::Value;

use crate::coverage::{Candidate, Confirmation, confirmation_current, covered};
use crate::helper::AuthKind;
use crate::hub::VerifiedState;
use crate::safety::{ExpectedSafety, MalformedSafety, UnverifiedReason};

/// How a containment request is authorised at the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubAuth {
    /// Policy permit (unattended).
    Policy,
    /// Human approval.
    Human {
        /// Step-up done.
        step_up: bool,
        /// Typed confirmation done.
        typed_confirmation: bool,
    },
}

/// What the hub decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Verified path: dispatch with the full identity.
    Verified(ExpectedSafety),
    /// Unverified human path, with the paths listed as not protected.
    UnverifiedHuman {
        /// Dispatch value (carries no identity).
        expected: ExpectedSafety,
        /// Why coverage cannot be verified.
        reason: UnverifiedReason,
        /// Paths shown as not protected.
        unprotected_paths: Vec<String>,
        /// Audit event written.
        audit: &'static str,
    },
    /// Refused.
    Refused(&'static str),
}

/// Decide a containment request. `paths` are every discovered management path; `confirmations`
/// the stored ones.
#[must_use]
pub fn hub_decide(
    auth: HubAuth,
    gate: Result<&VerifiedState, UnverifiedReason>,
    paths: &[Candidate],
    confirmations: &[Confirmation],
    now: u64,
) -> Decision {
    // Reason the verified path is unavailable, if any.
    let blocker: Option<UnverifiedReason> = match gate {
        Err(r) => Some(r),
        Ok(vs) => {
            if let Some(c) = confirmations.iter().find_map(|c| confirmation_current(c, Ok(vs), now).err()) {
                Some(c)
            } else if paths.iter().any(|p| !covered(vs, p)) {
                Some(UnverifiedReason::PathUncovered)
            } else {
                None
            }
        }
    };
    match (auth, blocker, gate) {
        (_, None, Ok(vs)) => Decision::Verified(ExpectedSafety::Verified(vs.identity.clone())),
        (HubAuth::Policy, Some(_), _) => Decision::Refused("unattended_containment_refused"),
        (HubAuth::Human { step_up, typed_confirmation }, Some(reason), g) => {
            if !(step_up && typed_confirmation) {
                return Decision::Refused("step_up_and_typed_confirmation_required");
            }
            let unprotected_paths = paths
                .iter()
                .filter(|p| g.map_or(true, |vs| !covered(vs, p)))
                .map(|p| p.cidr.render())
                .collect();
            Decision::UnverifiedHuman {
                expected: ExpectedSafety::Unverified(reason),
                reason,
                unprotected_paths,
                audit: "containment.unverified_approved",
            }
        }
        (_, None, Err(_)) => Decision::Refused("inconsistent"),
    }
}

/// The approval card text for an unverified human decision.
#[must_use]
pub fn approval_card(d: &Decision) -> String {
    match d {
        Decision::UnverifiedHuman { reason, unprotected_paths, .. } => {
            let mut s = format!(
                "Management-path coverage cannot be verified ({}).\n",
                reason.token()
            );
            for p in unprotected_paths {
                s.push_str(&format!("Not protected: {p}\n"));
            }
            s
        }
        Decision::Verified(_) => "Coverage verified.\n".to_owned(),
        Decision::Refused(r) => format!("Refused: {r}\n"),
    }
}

/// The agent's own checks before it calls the helper.
///
/// # Errors
/// `Err(malformed_safety)` for a missing or malformed value, `Ok(Err(..))` for the policy refusal.
pub fn agent_check(expected: Option<&Value>, auth: AuthKind) -> Result<Result<ExpectedSafety, &'static str>, MalformedSafety> {
    let v = expected.ok_or_else(|| MalformedSafety("missing expected_safety".into()))?;
    let e = ExpectedSafety::parse(v)?;
    if auth == AuthKind::Policy && matches!(e, ExpectedSafety::Unverified(_)) {
        return Ok(Err("unverified_requires_human"));
    }
    Ok(Ok(e))
}
