//! The request handler: serves `run_tool`, `run_skill` and `run_check` from the hub.
//!
//! Requests never run on the link task. At most [`MAX_CONCURRENT`] run at once; the link
//! refuses more than its queue holds as busy. Every request is cut off at its deadline,
//! and tool output is redacted, then capped at [`MAX_TOOL_OUTPUT`].

mod overview;
mod skills;
mod tools;

use std::sync::Arc;

use domovoi_checks::commands::Commands;
use domovoi_checks::containers::ContainerSource;
use domovoi_checks::system::FsStats;
use domovoi_core::ports::{Clock, HostFs, ProcessExecutor, Redactor};
use domovoi_core::{HostId, Risk, SignalStatus};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    CheckObservation, CheckResult, ErrorMessage, RunCheck, SkillFailure, SkillFailureCode,
    SkillOutcome, SkillResult, ToolFailure, ToolFailureCode, ToolOutcome, ToolResult, ToolSuccess,
};
use domovoi_runner::SkillCatalog;
use jiff::Timestamp;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub use self::overview::parse_loadavg;
pub use self::skills::{run_read_skill, run_skill};
pub use self::tools::{EVENTS_CHECK, run_tool};
use crate::checks::CheckSet;
use crate::link::{IncomingRequest, LinkHandle, RequestKind};
use crate::scheduler::CheckEnv;
use crate::store::CheckStore;

/// Most requests running at once.
pub const MAX_CONCURRENT: usize = 4;
/// Most bytes of text a tool returns.
pub const MAX_TOOL_OUTPUT: usize = 64 * 1024;

/// Why a tool failed, as sent to the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFail {
    /// The failure code.
    pub code: ToolFailureCode,
    /// A short explanation.
    pub message: String,
}

impl ToolFail {
    /// A failure with `code`.
    pub fn new(code: ToolFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Everything requests are served from.
pub struct RequestCtx {
    /// This host, for plan hashes.
    pub host_id: HostId,
    /// The local skill catalogue, the only skills that can run.
    pub catalog: Arc<SkillCatalog>,
    /// `[policy] allow_risk`.
    pub allow_risk: Vec<Risk>,
    /// Runs skills and commands (the runner's sandbox in production).
    pub exec: Arc<dyn ProcessExecutor>,
    /// Scrubs secrets from output.
    pub redactor: Arc<dyn Redactor>,
    /// Time, for deadlines.
    pub clock: Arc<dyn Clock>,
    /// The host's files.
    pub fs: Arc<dyn HostFs>,
    /// Filesystem usage for the overview.
    pub fs_stats: Arc<dyn FsStats>,
    /// Command templates.
    pub commands: Arc<Commands>,
    /// The container runtime, when Docker is enabled.
    pub containers: Option<Arc<dyn ContainerSource>>,
    /// The checks `run_check` may run.
    pub checks: Arc<dyn CheckSet>,
    /// What checks run with.
    pub check_env: CheckEnv,
    /// Check states.
    pub store: Arc<CheckStore>,
}

/// Cuts `text` to at most `max` bytes on a character boundary.
#[must_use]
pub fn cap(mut text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// Redacts, then caps, a tool's output.
#[must_use]
pub fn finish(redactor: &dyn Redactor, text: &str, truncated: bool) -> ToolSuccess {
    let (text, cut) = cap(redactor.redact(text), MAX_TOOL_OUTPUT);
    ToolSuccess {
        text,
        truncated: truncated || cut,
    }
}

/// The reply to a request refused because the handler is full.
#[must_use]
pub fn busy_reply(kind: &RequestKind) -> Message {
    const BUSY: &str = "the agent is busy; try again shortly";
    match kind {
        RequestKind::Tool(_) => Message::ToolResult(ToolResult {
            outcome: ToolOutcome::Err(ToolFailure {
                code: ToolFailureCode::Unavailable,
                message: BUSY.to_owned(),
            }),
        }),
        RequestKind::Skill(_) => Message::SkillResult(SkillResult {
            outcome: SkillOutcome::Err(SkillFailure {
                code: SkillFailureCode::Exec,
                message: BUSY.to_owned(),
            }),
        }),
        RequestKind::Check(_) => Message::Error(ErrorMessage {
            code: "busy".to_owned(),
            message: BUSY.to_owned(),
        }),
    }
}

async fn by_deadline<T>(
    clock: &dyn Clock,
    deadline: Timestamp,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        out = work => Some(out),
        () = clock.sleep_until(deadline) => None,
    }
}

async fn run_check(ctx: &RequestCtx, run: &RunCheck) -> Message {
    let Some(check) = ctx.checks.get(&run.check) else {
        return Message::Error(ErrorMessage {
            code: "unknown_check".to_owned(),
            message: format!("no check {} on this host", run.check),
        });
    };
    match ctx.check_env.run(check, &ctx.store).await {
        Ok(report) => {
            let observed_at = ctx.clock.now();
            let observations = report
                .observations
                .into_iter()
                .filter(|o| run.target.is_none() || o.target == run.target)
                .map(|o| CheckObservation {
                    target: o.target,
                    status: if o.firing {
                        SignalStatus::Firing
                    } else {
                        SignalStatus::Resolved
                    },
                    summary: o.summary,
                    evidence: o.evidence,
                    observed_at,
                })
                .collect();
            Message::CheckResult(CheckResult { observations })
        }
        Err(err) => Message::Error(ErrorMessage {
            code: "check_failed".to_owned(),
            message: err.to_string(),
        }),
    }
}

/// Serves one request and returns the reply.
pub async fn handle(ctx: &RequestCtx, kind: &RequestKind) -> Message {
    let clock = &*ctx.clock;
    match kind {
        RequestKind::Tool(run) => {
            let outcome = match by_deadline(clock, run.deadline, run_tool(ctx, &run.call)).await {
                Some(Ok((text, truncated))) => {
                    ToolOutcome::Ok(finish(&*ctx.redactor, &text, truncated))
                }
                Some(Err(fail)) => ToolOutcome::Err(ToolFailure {
                    code: fail.code,
                    message: ctx.redactor.redact(&fail.message),
                }),
                None => ToolOutcome::Err(ToolFailure {
                    code: ToolFailureCode::Timeout,
                    message: "the tool missed its deadline".to_owned(),
                }),
            };
            Message::ToolResult(ToolResult { outcome })
        }
        RequestKind::Skill(run) => {
            let outcome = by_deadline(clock, run.deadline, run_skill(ctx, run))
                .await
                .unwrap_or_else(|| {
                    SkillOutcome::Err(SkillFailure {
                        code: SkillFailureCode::Timeout,
                        message: "the skill missed its deadline".to_owned(),
                    })
                });
            Message::SkillResult(SkillResult { outcome })
        }
        RequestKind::Check(run) => by_deadline(clock, run.deadline, run_check(ctx, run))
            .await
            .unwrap_or_else(|| {
                Message::Error(ErrorMessage {
                    code: "timeout".to_owned(),
                    message: "the check missed its deadline".to_owned(),
                })
            }),
    }
}

/// Serves requests from `requests` until shutdown, at most [`MAX_CONCURRENT`] at once,
/// sending each reply through `link`.
pub async fn serve(
    ctx: Arc<RequestCtx>,
    mut requests: mpsc::Receiver<IncomingRequest>,
    link: LinkHandle,
    shutdown: CancellationToken,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let mut running = JoinSet::new();
    loop {
        let permit = tokio::select! {
            () = shutdown.cancelled() => break,
            permit = slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let request = tokio::select! {
            () = shutdown.cancelled() => break,
            request = requests.recv() => match request {
                Some(request) => request,
                None => break,
            },
        };
        let ctx = ctx.clone();
        let link = link.clone();
        running.spawn(async move {
            let reply = handle(&ctx, &request.kind).await;
            if !link.send(reply, Some(request.id)) {
                tracing::warn!("a reply was dropped; the outbox is full");
            }
            drop(permit);
        });
        while running.try_join_next().is_some() {}
    }
    running.abort_all();
}
