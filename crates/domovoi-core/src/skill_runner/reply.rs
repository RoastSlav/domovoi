//! What the runner answers.

use serde::{Deserialize, Serialize};

use crate::isolation::IsolationReport;

/// Exactly one reply per connection, or none after a cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerReply {
    /// The answer to a health request.
    Health(HealthReply),
    /// The result of a skill run, including a timeout.
    Run(RunReply),
    /// The runner's own observations, unevaluated.
    Probe(Box<IsolationReport>),
    /// The request was not carried out.
    Refused(Refusal),
}

/// The runner's identity and the roots it was started with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthReply {
    /// The protocol number the runner speaks.
    pub protocol: u32,
    /// The runner's build version.
    pub version: String,
    /// The skills directory the unit was rendered with.
    pub skills_root: String,
    /// The packages directory the unit was rendered with.
    pub packages_root: String,
}

/// The outcome of one skill process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReply {
    /// Exit status, absent when a signal ended the process.
    pub exit_code: Option<i32>,
    /// The signal that ended the process, if any.
    pub signal: Option<i32>,
    /// Captured standard output, unredacted: the agent redacts.
    pub stdout: String,
    /// Captured standard error, unredacted: the agent redacts.
    pub stderr: String,
    /// Whether either stream hit the output cap.
    pub truncated: bool,
    /// Whether the run hit its time limit.
    pub timed_out: bool,
    /// Wall-clock milliseconds the run took.
    pub duration_ms: u64,
    /// Hex SHA-256 of the argument vector that was run (64 lowercase hex characters).
    pub argv_hash: String,
}

/// A request that was not carried out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    /// The stable token for the reason.
    pub code: RefusalCode,
    /// One sentence; never an argument value.
    pub detail: String,
}

macro_rules! refusal_codes {
    ($($(#[$doc:meta])* $variant:ident => $token:literal,)+) => {
        /// Why the runner refused a request; each token is stable.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum RefusalCode {
            $($(#[$doc])* $variant,)+
        }

        impl RefusalCode {
            /// Every code, for tests that check uniqueness and spelling.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The stable snake-case token.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $token,)+
                }
            }
        }
    };
}

refusal_codes! {
    /// The frame was empty, too large, not JSON, had an unknown field, was truncated or was not read in time.
    BadRequest => "bad_request",
    /// The request's protocol number is not the runner's.
    ProtocolMismatch => "protocol_mismatch",
    /// The connecting uid may not make this kind of request.
    PeerNotAllowed => "peer_not_allowed",
    /// The start check found supplementary groups on the runner account.
    RunnerGroupsExtra => "runner_groups_extra",
    /// The start check found capabilities or a missing `NoNewPrivs`.
    RunnerPrivileges => "runner_privileges",
    /// No such skill folder under the root, or it resolves outside the root.
    SkillNotFound => "skill_not_found",
    /// The manifest or entry script failed a catalogue check.
    SkillInvalid => "skill_invalid",
    /// The recomputed implementation hash differs from the request's.
    ImplHashMismatch => "impl_hash_mismatch",
    /// The runner's own argument validation failed.
    ArgsInvalid => "args_invalid",
    /// The manifest names a `run_as` the runner does not support.
    RunAsUnsupported => "run_as_unsupported",
    /// The entry script changed between load and spawn.
    EntryChanged => "entry_changed",
    /// The process could not be started.
    SpawnFailed => "spawn_failed",
}
