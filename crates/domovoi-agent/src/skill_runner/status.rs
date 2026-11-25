//! What to report to the hub about the skill runner, from a fresh `health` exchange.

use std::path::PathBuf;

use domovoi_core::ports::Clock;
use domovoi_core::skill_runner::{HealthReply, RUNNER_PROTOCOL, RefusalCode};
use domovoi_proto::messages::SkillRunnerStatus;
use jiff::SignedDuration;

use super::client::{ClientError, RunnerClient};
use crate::time::within;

/// The longest the whole probe may take, on the injected clock: a connect plus a reply, with room.
const PROBE_DEADLINE: SignedDuration = SignedDuration::from_secs(8);

/// Reason for a runner whose socket this account may not open.
pub const REASON_SOCKET_PERMISSION: &str = "runner_socket_permission";
/// Reason for a runner that speaks another protocol number.
pub const REASON_PROTOCOL_MISMATCH: &str = "runner_protocol_mismatch";
/// Reason for a runner started with other roots than `agent.toml` names.
pub const REASON_ROOTS_DIFFER: &str = "runner_roots_differ";
/// Reason for a runner that did not answer properly.
pub const REASON_UNREACHABLE: &str = "runner_unreachable";

/// The skills and packages directories `agent.toml` names, which the runner's unit must have been rendered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDirs {
    /// `[agent] skills_dir`.
    pub skills: PathBuf,
    /// `[agent] packages_dir`.
    pub packages: PathBuf,
}

impl ExpectedDirs {
    fn matches(&self, reply: &HealthReply) -> bool {
        self.skills.to_str() == Some(reply.skills_root.as_str())
            && self.packages.to_str() == Some(reply.packages_root.as_str())
    }
}

/// The status to report right now, from one `health` exchange with the runner.
///
/// Absent socket is `Absent`; the start-check refusals are `Unsupported`; a runner whose
/// roots differ from `expected` is an `Error`, as is everything else that is not a good answer.
pub async fn probe_runner_status(
    client: &RunnerClient,
    expected: &ExpectedDirs,
    clock: &dyn Clock,
) -> SkillRunnerStatus {
    match within(clock, PROBE_DEADLINE, client.health()).await {
        None => error(REASON_UNREACHABLE),
        Some(Ok(reply)) => from_health(&reply, expected),
        Some(Err(err)) => from_error(&err),
    }
}

fn from_health(reply: &HealthReply, expected: &ExpectedDirs) -> SkillRunnerStatus {
    if reply.protocol != RUNNER_PROTOCOL {
        error(REASON_PROTOCOL_MISMATCH)
    } else if !expected.matches(reply) {
        error(REASON_ROOTS_DIFFER)
    } else {
        SkillRunnerStatus::Ok {
            version: reply.version.clone(),
        }
    }
}

/// The status a failed exchange stands for.
#[must_use]
pub fn from_error(err: &ClientError) -> SkillRunnerStatus {
    match err {
        ClientError::Absent => SkillRunnerStatus::Absent,
        ClientError::PermissionDenied => error(REASON_SOCKET_PERMISSION),
        ClientError::Refused(refusal) => match refusal.code {
            RefusalCode::RunnerGroupsExtra | RefusalCode::RunnerPrivileges => {
                SkillRunnerStatus::Unsupported {
                    reason: refusal.code.as_str().to_owned(),
                }
            }
            RefusalCode::ProtocolMismatch => error(REASON_PROTOCOL_MISMATCH),
            _ => error(REASON_UNREACHABLE),
        },
        ClientError::Connect(_)
        | ClientError::ClosedWithoutReply
        | ClientError::TimedOut
        | ClientError::Malformed(_) => error(REASON_UNREACHABLE),
    }
}

fn error(reason: &str) -> SkillRunnerStatus {
    SkillRunnerStatus::Error {
        reason: reason.to_owned(),
    }
}
