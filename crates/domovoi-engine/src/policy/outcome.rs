//! What a policy says about one request, and why.

use serde::{Deserialize, Serialize};

/// What the policy says about one request. Three authorization outcomes plus the hard-limit
/// outcome, which authorizes nothing.
///
/// The declaration order **is** the severity order the hard limits rely on:
/// `Allow < RequireApproval < Unavailable < Deny`. Derived `Ord` follows declaration order,
/// so moving a variant silently changes hard-limit monotonicity. Do not reorder these four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum Outcome {
    /// Unattended; a permit may be issued.
    Allow,
    /// Offered to the owner.
    RequireApproval,
    /// The evaluator could not establish a safety precondition. Nothing may execute and
    /// nothing is offered: this is not an authorization outcome, so it is never turned into
    /// a permit and never shown as approvable. No rule can produce it.
    Unavailable(UnavailableReason),
    /// Not offered. The owner must change the policy; there is no bypass.
    Deny,
}

impl Outcome {
    /// The serde name of the outcome, for error messages.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::RequireApproval => "require_approval",
            Self::Unavailable(_) => "unavailable",
            Self::Deny => "deny",
        }
    }
}

/// Why a hard limit produced `Unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The helper cannot compute its effective safety set.
    SafetyUnknown,
}

/// Whether the owner must type a confirmation phrase to approve the step.
///
/// This is a requirement carried in the trace, not an approval: the hub enforces it where
/// the owner approves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationRequirement {
    /// A plain approval is enough.
    None,
    /// The owner must type the confirmation.
    Typed,
}

/// Why the ordinary resolution produced its outcome.
///
/// These five cover every state: the winners' target intersection always contains the
/// request's target, so there is no "empty intersection" case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReason {
    /// No rule matched; the effect-class default applied.
    Default,
    /// Every winner agreed on one outcome.
    SingleWinner,
    /// Equal-scope winners disagreed; `Deny` won.
    EqualScopeDeny,
    /// Incomparable winners disagreed; the safer outcome won.
    IncomparableFallback,
    /// A matching locked `Deny` overrode the ordinary resolution.
    LockedDeny,
}

/// The hard limits, in application order. Declaration order is the trace order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardLimit {
    /// A destructive capability is never unattended.
    DestructiveNeverAllowed,
    /// The target is neither pinned nor named by a winning rule.
    TargetNotPinned,
    /// A declared safety fact is not known.
    SafetyFactUnknown,
    /// The host's unattended ceiling does not cover the request.
    CeilingDoesNotCover,
    /// No verified helper attestation for a containment request.
    ContainmentUnverified,
    /// A discovered management path is not protected.
    ManagementPathUncovered,
    /// The containment target overlaps the verified safety set.
    ContainmentTargetExcluded,
    /// The helper cannot compute its safety set.
    HelperSafetyIncomplete,
    /// Autonomy is paused or the circuit is broken.
    AutonomyInactive,
    /// The host's agent runs as root.
    AgentRunsAsRoot,
}
