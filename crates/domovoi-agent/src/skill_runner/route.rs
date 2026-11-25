//! How the agent executes a skill: through the runner, or (root agents only) in its own
//! process. This is the only code that decides which.

use std::sync::Arc;

use domovoi_core::identity::SKILL_ACCOUNT;
use domovoi_core::ports::{Clock, ProcessExecutor, SkillOutput};
use domovoi_core::skill_runner::{
    RUNNER_PROTOCOL, Refusal, RefusalCode, RunReply, RunRequest, SkillSource,
};
use domovoi_core::{ExecutionId, ImplHash, ValidatedArgs};
use domovoi_proto::messages::SkillRunnerStatus;
use domovoi_runner::{LoadedSkill, RunError, SkillCatalog};
use jiff::SignedDuration;
use tokio::sync::Semaphore;

use super::client::{ClientError, RunnerClient};
use super::state::RunnerState;
use super::status::{ExpectedDirs, REASON_UNREACHABLE, from_error, probe_runner_status};
use crate::requests::cap;
use crate::time::within;

/// How many skills may run through the runner at once. The socket unit accepts 16
/// connections; health and probe need room beside the runs.
pub const MAX_CONCURRENT_RUNS: usize = 8;

/// Time beyond a skill's own timeout that the agent waits for the runner's reply: the runner's
/// kill grace, the output drain and a margin. The runner's own limits come first.
const REPLY_MARGIN: SignedDuration = SignedDuration::from_secs(30);

/// Why a skill did not run to a result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    /// The runner is not usable; the text is the status reason or the transport failure.
    #[error("skill_runner_unavailable: {0}")]
    RunnerUnavailable(String),
    /// The manifest names a user other than the skill account.
    #[error("run_as_unsupported: the skill names run_as {run_as}")]
    RunAsUnsupported {
        /// The `run_as` value of the manifest.
        run_as: String,
    },
    /// The runner refused the request.
    #[error("{}: {detail}", .code.as_str())]
    Refused {
        /// The runner's stable token.
        code: RefusalCode,
        /// The runner's one-sentence reason.
        detail: String,
    },
    /// The skill hit its time limit; the output holds what it printed.
    #[error("the skill timed out")]
    TimedOut(SkillOutput),
    /// The skill could not be started or checked on the legacy path.
    #[error("{0}")]
    Exec(String),
}

/// How the agent executes a skill.
#[derive(Clone)]
pub enum SkillRoute {
    /// The agent spawns the skill itself. Only an agent running as root, where the privilege
    /// boundary is not in effect, is given this route.
    Legacy(Arc<dyn ProcessExecutor>),
    /// The skill runs as the skill account, through the runner socket.
    Runner(Arc<RunnerRoute>),
}

impl SkillRoute {
    /// Runs `skill` with `args` by this route.
    pub async fn run(
        &self,
        skill: &LoadedSkill,
        catalog: &SkillCatalog,
        args: &ValidatedArgs,
    ) -> Result<SkillOutput, RouteError> {
        match self {
            Self::Legacy(executor) => match catalog.run_skill(skill, args, &**executor).await {
                Ok(output) => Ok(output),
                Err(RunError::TimedOut { output }) => Err(RouteError::TimedOut(output)),
                Err(err) => Err(RouteError::Exec(err.to_string())),
            },
            Self::Runner(route) => route.run(skill, args).await,
        }
    }
}

/// The runner route: the client, the shared status and the concurrency limit.
pub struct RunnerRoute {
    client: RunnerClient,
    state: Arc<RunnerState>,
    expected: ExpectedDirs,
    clock: Arc<dyn Clock>,
    permits: Semaphore,
}

impl RunnerRoute {
    /// A route over `client` that expects the runner to have been started with `expected`.
    /// Its status starts as `Absent` until the first probe.
    #[must_use]
    pub fn new(client: RunnerClient, expected: ExpectedDirs, clock: Arc<dyn Clock>) -> Self {
        Self {
            client,
            state: Arc::new(RunnerState::new()),
            expected,
            clock,
            permits: Semaphore::new(MAX_CONCURRENT_RUNS),
        }
    }

    /// The socket client, for probes.
    #[must_use]
    pub fn client(&self) -> &RunnerClient {
        &self.client
    }

    /// The status shared with the probe task.
    #[must_use]
    pub fn state(&self) -> &Arc<RunnerState> {
        &self.state
    }

    /// Probes the runner now, records the answer in the shared status and returns it.
    pub async fn refresh(&self) -> SkillRunnerStatus {
        let status = probe_runner_status(&self.client, &self.expected, &*self.clock).await;
        self.state.set(status.clone());
        status
    }

    /// Runs one skill through the runner, never locally.
    pub async fn run(
        &self,
        skill: &LoadedSkill,
        args: &ValidatedArgs,
    ) -> Result<SkillOutput, RouteError> {
        let manifest = skill.manifest();
        if let Some(user) = manifest.run_as.as_deref()
            && user != SKILL_ACCOUNT
        {
            return Err(RouteError::RunAsUnsupported {
                run_as: user.to_owned(),
            });
        }
        let status = self.state.status();
        if !matches!(status, SkillRunnerStatus::Ok { .. }) {
            return Err(RouteError::RunnerUnavailable(reason_of(&status)));
        }
        let limit = manifest
            .timeout
            .checked_add(REPLY_MARGIN)
            .unwrap_or(REPLY_MARGIN);
        let request = RunRequest {
            protocol: RUNNER_PROTOCOL,
            execution_id: ExecutionId::new(),
            source: SkillSource::SkillsDir,
            skill: manifest.name.clone(),
            impl_hash: ImplHash::from_parts(skill.manifest_hash(), skill.entry_script_digest()),
            args: args.to_map(),
            timeout_secs: u32::try_from(manifest.timeout.as_secs().max(1)).unwrap_or(u32::MAX),
            max_output_bytes: manifest.max_output_bytes,
        };
        let Some(Ok(_permit)) = within(&*self.clock, limit, self.permits.acquire()).await else {
            return Err(RouteError::RunnerUnavailable("runner_busy".to_owned()));
        };
        let outcome = within(&*self.clock, limit, self.client.run(request)).await;
        match outcome {
            None => Err(self.transport_failed(&ClientError::TimedOut)),
            Some(Ok(reply)) => self.finish(reply, manifest.max_output_bytes),
            Some(Err(ClientError::Refused(refusal))) => Err(self.refused(refusal)),
            Some(Err(err)) => Err(self.transport_failed(&err)),
        }
    }

    fn finish(&self, reply: RunReply, max_output_bytes: u32) -> Result<SkillOutput, RouteError> {
        let Some(argv_hash) = parse_hash(&reply.argv_hash) else {
            return Err(self.transport_failed(&ClientError::Malformed(
                "an argument hash that is not 64 hex digits".to_owned(),
            )));
        };
        let max = max_output_bytes as usize;
        let (stdout, cut_out) = cap(reply.stdout, max);
        let (stderr, cut_err) = cap(reply.stderr, max);
        let output = SkillOutput {
            exit_code: reply.exit_code,
            stdout,
            stderr,
            truncated: reply.truncated || cut_out || cut_err,
            duration_ms: reply.duration_ms,
            argv_hash,
        };
        if reply.timed_out {
            Err(RouteError::TimedOut(output))
        } else {
            Ok(output)
        }
    }

    fn refused(&self, refusal: Refusal) -> RouteError {
        if matches!(
            refusal.code,
            RefusalCode::RunnerGroupsExtra
                | RefusalCode::RunnerPrivileges
                | RefusalCode::ProtocolMismatch
        ) {
            self.state
                .mark_error(from_error(&ClientError::Refused(refusal.clone())));
        }
        RouteError::Refused {
            code: refusal.code,
            detail: refusal.detail,
        }
    }

    /// Records a transport failure as the new status, wakes the probe task and reports it.
    fn transport_failed(&self, err: &ClientError) -> RouteError {
        let status = from_error(err);
        let reason = reason_of(&status);
        self.state.mark_error(status);
        RouteError::RunnerUnavailable(reason)
    }
}

/// The reason text of a status that is not `Ok`.
fn reason_of(status: &SkillRunnerStatus) -> String {
    match status {
        SkillRunnerStatus::Absent => "absent".to_owned(),
        SkillRunnerStatus::Unsupported { reason } | SkillRunnerStatus::Error { reason } => {
            reason.clone()
        }
        SkillRunnerStatus::Ok { .. } => REASON_UNREACHABLE.to_owned(),
    }
}

fn parse_hash(text: &str) -> Option<[u8; 32]> {
    hex::decode(text).ok()?.try_into().ok()
}
