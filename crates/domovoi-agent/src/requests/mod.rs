//! The request handler: serves `run_tool`, `run_skill`, `run_check` and `get_logs`
//! from the hub.
//!
//! Requests never run on the link task. At most [`MAX_CONCURRENT`] run at once; the link
//! refuses more than its queue holds as busy. Every request is cut off at its deadline,
//! and tool output is redacted, then capped at [`MAX_TOOL_OUTPUT`].

mod agent_read;
mod attest;
mod binary;
mod capability;
mod catalog;
mod firewall;
mod overview;
mod package_skills;
mod skill_journal;
mod skills;
mod tools;
mod unit;

use std::sync::Arc;

use domovoi_checks::commands::Commands;
use domovoi_checks::containers::ContainerSource;
use domovoi_checks::system::FsStats;
use domovoi_core::ports::{
    Clock, HostFs, LogLine, LogSource, PrivilegedReads, ProcessExecutor, Redactor,
};
use domovoi_core::{HostId, RequestId, Risk, SignalStatus};
use domovoi_proto::Message;
use domovoi_proto::Open;
use domovoi_proto::messages::{
    ActionState, ActionStatusAnswer, ActionStatusAnswers, ActionStatusQuery, CheckObservation,
    CheckResult, ErrorMessage, GetCatalog, Logs, OutcomeUnknownReason, RefusalCode, Refused,
    RunCheck, SkillFailure, SkillFailureCode, SkillOutcome, SkillResult, ToolFailure,
    ToolFailureCode, ToolOutcome, ToolResult, ToolSuccess,
};
use domovoi_runner::SkillCatalog;
use jiff::Timestamp;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub use self::attest::attest_reply;
pub use self::catalog::{catalog_entries, catalog_entries_with, page};
pub use self::overview::parse_loadavg;
pub use self::package_skills::Found;
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
/// Most log lines a `get_logs` reply carries, whatever the hub asked for.
pub const MAX_LOG_LINES: usize = 2000;
/// Most execution ids one `action_status` query may carry.
pub const MAX_STATUS_IDS: usize = 64;

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
    /// Runs tools and check commands (the runner crate's sandbox in production).
    pub exec: Arc<dyn ProcessExecutor>,
    /// Where skills run: the runner, or the agent's own process for a root agent.
    pub skills: crate::skill_runner::SkillRoute,
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
    /// The log lines of this process, for `get_logs`; `None` answers with none.
    pub logs: Option<Arc<dyn LogSource>>,
    /// The durable action journal; `None` refuses journal-backed requests.
    pub journal: Option<Arc<crate::journal::Journal>>,
    /// The package service; `None` without a `[packages]` section.
    pub packages: Option<Arc<crate::packages::PackageService>>,
    /// The helper's socket, for `attest_safety`.
    pub helper: crate::helper_client::HelperRelay,
    /// Kernel-log and listening-socket reads served by the helper when the privilege boundary
    /// is in effect; `None` keeps the direct paths.
    pub privileged: Option<Arc<dyn PrivilegedReads>>,
    /// The unattended ceiling from `agent.toml [unattended]`; `None` accepts no
    /// policy-labelled request.
    pub ceiling: Option<Arc<crate::unattended::Ceiling>>,
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

/// Re-scrubs a log line's message before the hub sees it: the ring scrubs at capture
/// time already, but the wire copy must not rely on that.
#[must_use]
fn redact_line(redactor: &dyn Redactor, line: LogLine) -> LogLine {
    LogLine {
        at: line.at,
        level: line.level,
        target: line.target,
        message: redactor.redact(&line.message),
    }
}

/// The `get_logs` reply for the redacted `window`: the biggest chunk from
/// `from` already-delivered lines on whose whole encoded frame the link may
/// send, which `encode` refuses past its limit. Lines are added until it
/// answers within the frame, and `has_more` tells the hub to ask again with a
/// bigger `offset`. A line too big to fit even alone is dropped rather than
/// dropping the link.
#[must_use]
fn chunk_logs(window: Vec<LogLine>, from: usize) -> Message {
    let probe = RequestId::new();
    let mut lo = from.min(window.len());
    let mut hi = window.len();
    loop {
        let reply = Message::Logs(Logs {
            lines: window[lo..hi].to_vec(),
            has_more: hi < window.len(),
        });
        if domovoi_proto::encode(&reply, probe, Some(probe)).is_ok() {
            return reply;
        }
        if hi - lo > 1 {
            hi = lo + (hi - lo) / 2;
        } else {
            lo += 1;
        }
    }
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
        RequestKind::Logs(_)
        | RequestKind::Capability(_)
        | RequestKind::Catalog(_)
        | RequestKind::Attest(_)
        | RequestKind::ActionStatus(_) => Message::Error(ErrorMessage {
            code: "busy".to_owned(),
            message: BUSY.to_owned(),
        }),
    }
}

/// A refusal that ends one execution without ending the session.
fn refused(code: &str, known: RefusalCode, detail: impl Into<String>) -> Message {
    Message::Refused(Refused {
        code: Open::new(code, Some(known)),
        detail: detail.into(),
        retry_after: None,
        scope: None,
    })
}

/// Answers `get_catalog` with one page of the agent's implementation list (6.4.1).
///
/// A page index past [`catalog::MAX_PAGE_INDEX`] is refused as `invalid` instead of answered
/// empty: an empty page ends the hub's walk, and ending a walk that never read the list would
/// let the hub report a verified catalogue.
fn catalog_reply(ctx: &RequestCtx, query: &GetCatalog) -> Message {
    if query.page > catalog::MAX_PAGE_INDEX {
        let width = catalog::MAX_PAGE_ENTRIES;
        let wanted = query.page;
        return Message::Error(ErrorMessage {
            code: "invalid".to_owned(),
            message: format!("the page {wanted} cannot be addressed at {width} entries per page"),
        });
    }
    Message::CatalogPage(catalog::page(
        &catalog_entries_with(
            &ctx.catalog,
            ctx.package_skills_surface(),
            &ctx.enabled_package_skill_names(),
        ),
        query,
    ))
}

/// Why an `action_status` query is answered conservatively.
///
/// The journal can only prove that a run never arrived when every execution path records
/// its arrival. A read skill run through `run_skill` records nothing in the journal (skills
/// that change the host do), so an id it does not know may still have run. Until executions are journal-backed, absence is
/// reported as an unknowable outcome and never as `not_found`, so the hub cannot turn it
/// into "nothing was started". It is also why this agent does not advertise
/// `AUTHORITATIVE_ABSENCE` on `hello`: it cannot make that promise while a run can execute
/// without a journal record. The hub fails closed for every agent that does not promise it.
/// This also removes the need to order a status query behind a request whose acceptance is
/// still being recorded.
fn conservative(answers: ActionStatusAnswers) -> ActionStatusAnswers {
    ActionStatusAnswers {
        epoch: answers.epoch,
        answers: answers
            .answers
            .into_iter()
            .map(|answer| match answer.state {
                ActionState::NotFound => ActionStatusAnswer {
                    state: ActionState::OutcomeUnknown {
                        reason: Open::new("journal_lost", Some(OutcomeUnknownReason::JournalLost)),
                    },
                    ..answer
                },
                _ => answer,
            })
            .collect(),
    }
}

fn action_status(ctx: &RequestCtx, query: &ActionStatusQuery) -> Message {
    if query.execution_ids.len() > MAX_STATUS_IDS {
        return Message::Error(ErrorMessage {
            code: "bad_request".to_owned(),
            message: format!(
                "an action_status query carries at most {MAX_STATUS_IDS} execution ids"
            ),
        });
    }
    match &ctx.journal {
        Some(journal) => match journal.action_status(query) {
            Ok(answers) => Message::ActionStatusAnswers(conservative(answers)),
            Err(err) => Message::Error(ErrorMessage {
                code: "journal_error".to_owned(),
                message: ctx.redactor.redact(&err.to_string()),
            }),
        },
        None => Message::Error(ErrorMessage {
            code: "unavailable".to_owned(),
            message: "there is no action journal on this agent".to_owned(),
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
        RequestKind::Skill(run) => by_deadline(clock, run.deadline, run_skill(ctx, run))
            .await
            .unwrap_or_else(|| {
                Message::SkillResult(SkillResult {
                    outcome: SkillOutcome::Err(SkillFailure {
                        code: SkillFailureCode::Timeout,
                        message: "the skill missed its deadline".to_owned(),
                    }),
                })
            }),
        RequestKind::Check(run) => by_deadline(clock, run.deadline, run_check(ctx, run))
            .await
            .unwrap_or_else(|| {
                Message::Error(ErrorMessage {
                    code: "timeout".to_owned(),
                    message: "the check missed its deadline".to_owned(),
                })
            }),
        RequestKind::Logs(get) => {
            let max = usize::from(get.lines).min(MAX_LOG_LINES);
            let lines = match &ctx.logs {
                Some(logs) => logs.tail(max, &get.min_level),
                None => Vec::new(),
            };
            let window = lines
                .into_iter()
                .map(|line| redact_line(&*ctx.redactor, line))
                .collect();
            chunk_logs(window, usize::from(get.offset))
        }
        RequestKind::Capability(run) => capability::run_capability(ctx, run).await,
        RequestKind::Catalog(get) => catalog_reply(ctx, get),
        RequestKind::ActionStatus(query) => action_status(ctx, query),
        RequestKind::Attest(challenge) => attest_reply(ctx, challenge).await,
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

#[cfg(test)]
mod tests {
    use domovoi_proto::messages::{
        ActionState, ActionStatusAnswer, ActionStatusAnswers, OutcomeUnknownReason,
    };
    use uuid::Uuid;

    use super::conservative;

    #[test]
    fn an_absent_execution_is_reported_as_unknowable_not_as_missing() {
        let known = Uuid::now_v7();
        let absent = Uuid::now_v7();
        let answers = conservative(ActionStatusAnswers {
            epoch: Uuid::nil(),
            answers: vec![
                ActionStatusAnswer {
                    execution_id: absent,
                    state: ActionState::NotFound,
                },
                ActionStatusAnswer {
                    execution_id: known,
                    state: ActionState::Running,
                },
            ],
        });
        assert!(
            matches!(
                &answers.answers[0].state,
                ActionState::OutcomeUnknown { reason }
                    if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
            ),
            "the legacy execution path keeps no journal record, so absence is not proof",
        );
        assert!(
            matches!(answers.answers[1].state, ActionState::Running),
            "what the journal does know is reported unchanged",
        );
    }
}
