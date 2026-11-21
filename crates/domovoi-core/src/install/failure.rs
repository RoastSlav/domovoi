//! Why a target did not finish, as stored text.

use crate::package::PackageFailureCode;

/// Failure codes the hub itself produces.
pub mod codes {
    /// The host's agent is not connected or has no usable session.
    pub const HOST_UNAVAILABLE: &str = "host_unavailable";
    /// The agent speaks another protocol version.
    pub const AGENT_INCOMPATIBLE: &str = "agent_incompatible";
    /// The agent does not offer the package capability.
    pub const NOT_ADVERTISED: &str = "not_advertised";
    /// The archive file is gone from the hub's store.
    pub const ARTIFACT_MISSING: &str = "artifact_missing";
    /// The archive on disk no longer matches its recorded digest.
    pub const ARTIFACT_CORRUPT: &str = "artifact_corrupt";
    /// A transfer to the agent failed.
    pub const TRANSFER_FAILED: &str = "transfer_failed";
    /// The agent session changed before the request was handed over.
    pub const SESSION_CHANGED: &str = "session_changed";
    /// The request could not be handed to the agent.
    pub const HANDOFF_FAILED: &str = "handoff_failed";
    /// The approval ran out before the work started.
    pub const APPROVAL_EXPIRED: &str = "approval_expired";
    /// The agent's success outputs differ from what the job asked for.
    pub const RESULT_MISMATCH: &str = "result_mismatch";
    /// The hub restarted while the target was transferring.
    pub const HUB_RESTARTED: &str = "hub_restarted";
    /// The agent refused or failed with a message the hub does not recognise.
    pub const AGENT_REFUSED: &str = "agent_refused";
    /// The agent's journal has no record of the execution.
    pub const NOT_RUN: &str = "not_run";
    /// The agent's journal epoch changed since the request was handed over.
    pub const EPOCH_CHANGED: &str = "epoch_changed";
    /// The request was handed over before the hub recorded an epoch.
    pub const EPOCH_UNKNOWN: &str = "epoch_unknown";
}

/// Refusal codes the agent's capability layer answers with, passed through verbatim.
pub const REFUSAL_CODES: [&str; 5] = [
    "contract_unknown",
    "allow_risk_denied",
    "authorization_expired",
    "rate_limited",
    "journal_corrupt",
];

/// The longest failure message stored, in characters.
pub const MAX_FAILURE_MESSAGE_CHARS: usize = 500;

/// Why a target ended as it did: a `snake_case` code and a short message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFailure {
    code: String,
    message: String,
}

impl TargetFailure {
    /// A failure with `code` and `message`.
    ///
    /// A code that is not a lowercase token becomes `agent_refused`, and the message is cut
    /// to [`MAX_FAILURE_MESSAGE_CHARS`] characters, so stored text is always well formed.
    #[must_use]
    pub fn new(code: &str, message: &str) -> Self {
        let code = if is_token(code) {
            code
        } else {
            codes::AGENT_REFUSED
        };
        Self {
            code: code.to_owned(),
            message: truncate(message),
        }
    }

    /// Reads the agent's `"<code>: <detail>"` text.
    ///
    /// A package failure code or a refusal code passes through verbatim; anything else is
    /// `agent_refused` with the whole text kept.
    #[must_use]
    pub fn from_agent_message(text: &str) -> Self {
        if let Some((code, detail)) = text.split_once(':') {
            let code = code.trim();
            if is_agent_code(code) {
                return Self::new(code, detail.trim());
            }
        }
        Self::new(codes::AGENT_REFUSED, text)
    }

    /// The `snake_case` code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The message, at most [`MAX_FAILURE_MESSAGE_CHARS`] characters.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Whether `code` is one the agent can answer with: a package failure or a refusal.
fn is_agent_code(code: &str) -> bool {
    code.parse::<PackageFailureCode>().is_ok() || REFUSAL_CODES.contains(&code)
}

fn is_token(code: &str) -> bool {
    let mut chars = code.chars();
    code.len() <= 48
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn truncate(text: &str) -> String {
    text.chars().take(MAX_FAILURE_MESSAGE_CHARS).collect()
}
