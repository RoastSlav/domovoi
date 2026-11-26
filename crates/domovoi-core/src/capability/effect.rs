//! What a capability can do, where it runs and what verifying it means.

use serde::{Deserialize, Serialize};

use crate::skill::Risk;

/// What a capability can do to a host, ordered by severity.
///
/// The declaration order is the severity order the skill floor compares with, so
/// `Read < Mutate < Contain < Destructive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    /// Observes state and changes nothing.
    Read,
    /// Changes state reversibly or idempotently.
    Mutate,
    /// Reduces blast radius, for example a timed firewall block with a kernel-enforced expiry.
    Contain,
    /// Can lose data.
    Destructive,
}

impl EffectClass {
    /// Every class, in severity order.
    pub const ALL: [EffectClass; 4] = [
        EffectClass::Read,
        EffectClass::Mutate,
        EffectClass::Contain,
        EffectClass::Destructive,
    ];

    /// The wire and display name of this class.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Mutate => "mutate",
            Self::Contain => "contain",
            Self::Destructive => "destructive",
        }
    }

    /// Reads the declared class of a skill or catalogue entry, failing closed.
    ///
    /// Only the exact words `read`, `write` and `destructive` are known; `write` is the
    /// manifest word for [`Mutate`](Self::Mutate). Everything else, including `mutate`,
    /// `contain`, an empty string or an unknown word, is [`Destructive`](Self::Destructive):
    /// a provider cannot declare itself safe by inventing a class.
    #[must_use]
    pub fn from_declared_risk(text: &str) -> EffectClass {
        match text {
            "read" => Self::Read,
            "write" => Self::Mutate,
            "destructive" => Self::Destructive,
            _ => Self::Destructive,
        }
    }
}

impl From<Risk> for EffectClass {
    fn from(risk: Risk) -> Self {
        match risk {
            Risk::Read => Self::Read,
            Risk::Write => Self::Mutate,
            Risk::Destructive => Self::Destructive,
        }
    }
}

/// What kind of thing a capability acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// The capability has no target.
    None,
    /// The host itself.
    Host,
    /// One container.
    Container,
    /// One systemd unit.
    Unit,
    /// One external network address or prefix.
    NetworkSource,
}

/// Whether a capability's effect can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    /// The question does not apply, as for a read.
    NotApplicable,
    /// The effect can be undone.
    Reversible,
    /// The effect cannot be undone.
    Irreversible,
}

/// Whether running a capability twice has the same effect as running it once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    /// Running it again changes nothing further.
    Idempotent,
    /// A second run may change state again.
    NotIdempotent,
}

/// Where a capability executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPath {
    /// Answered by the hub from its own store.
    Hub,
    /// Run by the agent process itself.
    Agent,
    /// Run through the skill runner identity.
    SkillRunner,
    /// A typed privileged operation through the root helper.
    Helper,
}

/// How an operator or the hub can check that a capability worked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationHint {
    /// Nothing to verify, as for a read.
    None,
    /// Re-run the originating check after a delay.
    Recheck,
    /// The capability's own postcondition, described for the approval card.
    Postcondition {
        /// What should hold after the capability runs.
        description: String,
    },
}
