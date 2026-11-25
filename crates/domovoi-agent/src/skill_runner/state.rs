//! What the agent currently believes about the skill runner, shared by the route that runs
//! skills and the status task that probes it.

use std::sync::Mutex;

use domovoi_proto::messages::SkillRunnerStatus;
use tokio::sync::Notify;

/// The last known runner status plus a wake-up for the probe task.
///
/// The route reads it before every run and writes `Error` after a transport failure; the
/// status task writes every probe result and sleeps on [`RunnerState::woken`].
#[derive(Debug)]
pub struct RunnerState {
    status: Mutex<SkillRunnerStatus>,
    wake: Notify,
}

impl Default for RunnerState {
    fn default() -> Self {
        Self::new()
    }
}

impl RunnerState {
    /// A state that starts as `Absent` until the first probe says otherwise.
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: Mutex::new(SkillRunnerStatus::Absent),
            wake: Notify::new(),
        }
    }

    /// The status now. A poisoned lock reads as an error, never as `Ok`.
    #[must_use]
    pub fn status(&self) -> SkillRunnerStatus {
        match self.status.lock() {
            Ok(status) => status.clone(),
            Err(_) => SkillRunnerStatus::Error {
                reason: "runner_state_unavailable".to_owned(),
            },
        }
    }

    /// Replaces the status.
    pub fn set(&self, status: SkillRunnerStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }

    /// Whether skills may run now.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self.status(), SkillRunnerStatus::Ok { .. })
    }

    /// Records a transport failure and asks the probe task to look again at once.
    pub fn mark_error(&self, status: SkillRunnerStatus) {
        self.set(status);
        self.wake.notify_one();
    }

    /// Completes when a transport failure asked for an immediate probe.
    pub async fn woken(&self) {
        self.wake.notified().await;
    }
}
