//! Asks the skill runner for the self-test: the runner halves of ST-05, ST-06, ST-13 and ST-14.
//!
//! The helper crate decides; this file only reduces what the agent's socket client returned to
//! the closed observations the helper crate understands.

use std::path::Path;

use domovoi_agent::skill_runner::{ClientError, RunnerClient};
use domovoi_core::isolation::IsolationReport;
use domovoi_core::skill_runner::{HealthReply, RefusalCode};
use domovoi_helper::selftest::{
    HealthAnswer, HealthProbe, RunnerHealth, RunnerObservation, UnixHealthProbe,
};

/// What a `probe` exchange means for ST-05, ST-06 and ST-13.
///
/// The runner answers every request, `probe` included, with its start-check refusal once that
/// check failed; those two codes become the failures of ST-05 and ST-06. Every other error is an
/// unreachable runner and never a pass.
pub fn observation_from(result: Result<IsolationReport, ClientError>) -> RunnerObservation {
    match result {
        Ok(report) => RunnerObservation::Report(Box::new(report)),
        Err(ClientError::Absent) => RunnerObservation::NotInstalled,
        Err(ClientError::Refused(refusal)) => match refusal.code {
            RefusalCode::RunnerGroupsExtra => RunnerObservation::GroupsExtra(refusal.detail),
            RefusalCode::RunnerPrivileges => RunnerObservation::Privileges(refusal.detail),
            other => RunnerObservation::Unavailable(format!(
                "refused {}: {}",
                other.as_str(),
                refusal.detail
            )),
        },
        Err(other) => RunnerObservation::Unavailable(other.to_string()),
    }
}

/// What a `health` exchange means for the runner half of ST-14: a refusal still proves the
/// socket connects (ST-05 and ST-06 carry what it says).
pub fn health_from(result: Result<HealthReply, ClientError>) -> RunnerHealth {
    match result {
        Ok(_) | Err(ClientError::Refused(_)) => RunnerHealth::Answered,
        Err(ClientError::Absent) => RunnerHealth::NotInstalled,
        Err(other) => RunnerHealth::Unreachable(other.to_string()),
    }
}

/// The helper's health probe plus the runner's answer, asked beforehand because the client is
/// async and the probe port is not.
pub struct RunnerAwareProbe {
    helper: UnixHealthProbe,
    runner: RunnerHealth,
}

impl RunnerAwareProbe {
    /// A probe that reports `runner` for the runner half.
    pub fn new(runner: RunnerHealth) -> Self {
        Self {
            helper: UnixHealthProbe,
            runner,
        }
    }
}

impl HealthProbe for RunnerAwareProbe {
    fn health(&self, socket: &Path) -> Result<HealthAnswer, String> {
        self.helper.health(socket)
    }

    fn runner_health(&self) -> RunnerHealth {
        self.runner.clone()
    }
}

/// Root: the runner's own filesystem observations (`probe`).
pub async fn observe(client: &RunnerClient) -> RunnerObservation {
    observation_from(client.probe(None).await)
}

/// Any account: whether the runner socket answers `health`.
pub async fn health(client: &RunnerClient) -> RunnerHealth {
    health_from(client.health().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domovoi_core::skill_runner::Refusal;

    fn refused(code: RefusalCode) -> ClientError {
        ClientError::Refused(Refusal {
            code,
            detail: "start check".to_owned(),
        })
    }

    #[test]
    fn start_refusals_become_their_own_observations() {
        assert!(matches!(
            observation_from(Err(refused(RefusalCode::RunnerGroupsExtra))),
            RunnerObservation::GroupsExtra(_)
        ));
        assert!(matches!(
            observation_from(Err(refused(RefusalCode::RunnerPrivileges))),
            RunnerObservation::Privileges(_)
        ));
    }

    #[test]
    fn other_errors_are_never_a_report() {
        assert_eq!(
            observation_from(Err(ClientError::Absent)),
            RunnerObservation::NotInstalled
        );
        for error in [
            ClientError::PermissionDenied,
            ClientError::TimedOut,
            ClientError::ClosedWithoutReply,
            refused(RefusalCode::ProtocolMismatch),
        ] {
            assert!(matches!(
                observation_from(Err(error)),
                RunnerObservation::Unavailable(_)
            ));
        }
    }

    #[test]
    fn health_refusals_still_prove_the_socket_connects() {
        assert_eq!(
            health_from(Err(refused(RefusalCode::RunnerPrivileges))),
            RunnerHealth::Answered
        );
        assert_eq!(
            health_from(Err(ClientError::Absent)),
            RunnerHealth::NotInstalled
        );
        assert!(matches!(
            health_from(Err(ClientError::TimedOut)),
            RunnerHealth::Unreachable(_)
        ));
    }
}
