//! The hub side of an agent connection, and where signals enter the hub.

use async_trait::async_trait;
use jiff::Timestamp;

use crate::action::{JournalEpoch, PlanHash};
use crate::capability::catalog::{CatalogPageData, CatalogQuery};
use crate::id::{ActionId, AgentId, AgentSessionId, HostId, RequestId};
use crate::install::{
    CapabilityHandoff, PackageDispatch, PackageOutputs, TransferAnswer, TransferFrame,
};
use crate::ports::capability::HostView;
use crate::signal::{CheckKey, Evidence, Signal, SignalStatus, TargetKey};
use crate::skill::{SkillName, SkillSummary, ValidatedArgs};
use crate::triage::ReadToolCall;

/// What a read tool returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// The output text, already redacted by the agent.
    pub text: String,
    /// Whether the agent cut the output at its cap.
    pub truncated: bool,
}

/// An approved action sent to an agent for execution.
///
/// Only the action executor builds one, after the action reached `Dispatched` with a
/// matching plan hash. The agent re-validates `args` and `plan_hash` against its own
/// manifest before running anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDispatch {
    /// The action being executed.
    pub action: ActionId,
    /// The skill to run.
    pub skill: SkillName,
    /// The validated arguments.
    pub args: ValidatedArgs,
    /// The plan hash the approval bound to.
    pub plan_hash: PlanHash,
}

/// A skill request the hub has handed to an agent's session.
///
/// It exists from the moment the encoded frame is in that session's outgoing queue, which
/// is the point after which the agent may receive the execution and record it in its
/// journal. [`AgentLink::skill_result`] takes it and answers with what the run produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillHandoff {
    request: RequestId,
}

impl SkillHandoff {
    /// Wraps the id of the request frame the hub queued.
    #[must_use]
    pub const fn new(request: RequestId) -> Self {
        Self { request }
    }

    /// The id the queued frame was tagged with.
    #[must_use]
    pub const fn request(self) -> RequestId {
        self.request
    }
}

/// What a skill run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillOutput {
    /// Exit code, or `None` when killed.
    pub exit_code: Option<i32>,
    /// Standard output, redacted and capped.
    pub stdout: String,
    /// Standard error, redacted and capped.
    pub stderr: String,
    /// Whether either stream was cut.
    pub truncated: bool,
    /// Wall-clock run time in milliseconds.
    pub duration_ms: u64,
    /// SHA-256 of the argv the agent ran, for the audit log.
    pub argv_hash: [u8; 32],
}

/// The result of re-running one check on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckRecheck {
    /// Whether the condition still holds.
    pub status: SignalStatus,
    /// One-line description.
    pub summary: String,
    /// Fresh evidence.
    pub evidence: Evidence,
    /// When the check ran.
    pub observed_at: Timestamp,
}

/// What an agent's journal says about a run id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionAnswer {
    /// The run finished; `detail` is the agent's failure message when it failed.
    Finished {
        /// Whether the run's recorded result is a success.
        succeeded: bool,
        /// The agent's failure message, when the run failed.
        detail: Option<String>,
    },
    /// Still running in the agent instance that started it.
    Running,
    /// Positive evidence the request never reached the journal.
    NotFound,
    /// The journal cannot establish what happened.
    Unknown {
        /// Why.
        reason: UnknownReason,
    },
}

/// Why an outcome is unknown. The first three come from the agent's answer; the hub
/// decides the last two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// The journal recorded acceptance but never a start record.
    NoStartRecord,
    /// The agent restarted while the execution was recorded as started.
    AgentRestarted,
    /// The journal lost the record, or the hub's epoch is stale.
    JournalLost,
    /// The journal epoch the hub recorded at dispatch is not the live one.
    EpochChanged,
    /// The run was dispatched before the hub recorded an epoch.
    EpochUnknown,
}

/// One answer, keyed by the id that was asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswerEntry {
    /// The run the answer is about.
    pub execution: ActionId,
    /// What the journal knows.
    pub answer: ActionAnswer,
}

/// The agent's answers and the epoch it answered under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswers {
    /// The journal's actual epoch.
    pub epoch: JournalEpoch,
    /// One answer per requested id that the agent recognised.
    pub answers: Vec<ActionAnswerEntry>,
}

/// The coherent view of one authenticated agent session that a v2 plan is calculated from.
///
/// The session id is unique per connection; the journal epoch is not, because it can survive
/// a reconnection. Every field belongs to the same session and is read together by
/// [`AgentLink::snapshot`], so a plan cannot mix one session's skills with another session's
/// key or journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSnapshot {
    /// The authenticated connection this snapshot describes.
    pub session: AgentSessionId,
    /// The enrolled agent that authenticated this connection.
    pub agent: AgentId,
    /// The journal epoch this session announced.
    pub journal_epoch: JournalEpoch,
    /// The skills this session announced on its handshake.
    pub skills: Vec<SkillSummary>,
}

/// Whether an agent is connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// Connected now.
    Connected {
        /// Since when.
        since: Timestamp,
        /// Agent version from the handshake.
        version: String,
    },
    /// Not connected.
    Disconnected {
        /// When it was last connected, if ever.
        last_seen: Option<Timestamp>,
    },
}

/// Where a host's agent stands, as the action executor needs to know before it dispatches.
///
/// Unlike [`Presence`], this separates a host that has never been seen (**Unknown**) from
/// one whose agent connected and then went away (**Absent**), and reports a host whose
/// agent speaks another protocol version (**Incompatible**) instead of treating it as
/// merely offline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAvailability {
    /// The agent is connected now, since `since`.
    Known {
        /// When this session started.
        since: Timestamp,
    },
    /// The host has no agent session and never had one in this hub run.
    Unknown,
    /// The host's agent was connected and has been gone since `since`.
    Absent {
        /// When the last session ended.
        since: Timestamp,
    },
    /// The hub rejected the host's agent because it speaks another protocol version.
    Incompatible {
        /// The version the agent reported.
        reported: String,
    },
}

/// Why a request to an agent failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// The host has no connected agent.
    #[error("agent not connected")]
    NotConnected,
    /// The host's agent speaks another protocol version than this hub. Work on the host is
    /// unavailable until the agent is upgraded. Reason code: `agent_incompatible`.
    #[error("agent_incompatible: the agent speaks another protocol version; upgrade it")]
    Incompatible,
    /// The session the request was meant for is gone: the agent's journal epoch changed, so
    /// the request is not sent to whichever agent replaced it. Nothing was handed over.
    #[error("the agent's journal changed; the request was not sent")]
    SessionChanged,
    /// No answer before the deadline.
    #[error("timed out")]
    Timeout,
    /// The agent refused the request, for example after local re-validation.
    #[error("rejected by agent: {0}")]
    Rejected(String),
    /// The request was handed over but no usable answer came back: the link failed, the
    /// agent sent something other than the expected result, or the reply could not be read.
    /// This is not evidence that the run did not happen.
    #[error("result unavailable: {0}")]
    Indeterminate(String),
    /// The connection failed mid-request.
    #[error("transport error: {0}")]
    Transport(String),
}

/// Sends requests to connected agents. Every method fails fast with
/// [`LinkError::NotConnected`] when the host has no connected agent, with
/// [`LinkError::Incompatible`] when this hub rejected the agent's protocol, with
/// [`LinkError::SessionChanged`] when the request named a journal the live session no longer
/// has, and with [`LinkError::Timeout`] once `deadline` passes.
#[async_trait]
pub trait AgentLink: Send + Sync {
    /// Runs one read tool on `host`.
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError>;

    /// Hands one approved skill to `host`'s live session, which must still be the exact
    /// `expected_session` and the journal `expected_epoch` names, and returns once the frame
    /// is in that session's outgoing queue.
    ///
    /// Failing means no request was handed over: no frame was queued, so the run cannot have
    /// happened. Implementations must not answer with an error after the frame is queued;
    /// every later failure belongs to [`AgentLink::skill_result`], whose errors are not proof
    /// of anything. The executor settles an error here as a run that never started.
    /// [`LinkError::SessionChanged`] means the session or journal the action was dispatched
    /// under is gone, so the request is not sent to a replacement.
    async fn start_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError>;

    /// The coherent session snapshot for `host`, or `None` with no live session.
    ///
    /// The snapshot covers the authenticated session, the enrolled agent, the journal epoch
    /// and the announced skills. A caller that calculates a v2 plan from it pins all four to
    /// the session it later hands the request to.
    fn snapshot(&self, host: HostId) -> Option<AgentSnapshot>;

    /// Waits for the terminal answer to a skill request handed over by
    /// [`AgentLink::start_skill`].
    ///
    /// [`LinkError::Rejected`] is a confirmed terminal answer from the agent (it refused the
    /// run or reported a failure). Every other error, including a timeout or a dropped
    /// session, leaves the run's outcome unknown: it may have happened, so it is never
    /// recorded as a failure.
    async fn skill_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError>;

    /// Re-runs one check on `host`, for verification after an action.
    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError>;

    /// Current connection state of `host`'s agent.
    fn presence(&self, host: HostId) -> Presence;

    /// Whether the host's agent is reachable, was reachable and left, was never seen, or
    /// was rejected. Links that only know [`Presence`] get this default, which cannot
    /// tell a host that never connected from one that disconnected.
    async fn availability(&self, host: HostId) -> HostAvailability {
        match self.presence(host) {
            Presence::Connected { since, .. } => HostAvailability::Known { since },
            Presence::Disconnected {
                last_seen: Some(since),
            } => HostAvailability::Absent { since },
            Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
        }
    }

    /// The journal epoch the host's live session reported, or `None` with no live session.
    fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch>;

    /// What the host's agent knows about `ids` under `expected_epoch`. At most 64 ids.
    async fn action_status(
        &self,
        host: HostId,
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError>;

    /// Skills the agent announced on its last handshake; empty when never connected.
    fn skills(&self, host: HostId) -> Vec<SkillSummary>;

    /// What the host's live agent said about itself beyond presence, or `None` with no live
    /// session. Words and skills are read together, so one session's status can never be
    /// combined with another session's skills; a link that cannot answer `None` is treated as
    /// having no session rather than as reporting absence.
    fn host_view(&self, _host: HostId) -> Option<HostView> {
        None
    }

    /// The catalogue hash the host's agent announced on its last handshake, as 64 lowercase
    /// hex characters. `None` means no live session or no announcement, so a walk has nothing
    /// to verify against and stays unverified.
    fn catalog_hash(&self, _host: HostId) -> Option<String> {
        None
    }

    /// Asks the agent for one page of its capability catalogue (wire 1.2).
    ///
    /// The answer is contract ids and versions only. A link that cannot page, and an agent too
    /// old to answer (`unsupported`), both fail closed as an incomplete catalogue.
    async fn get_catalog(
        &self,
        _host: HostId,
        _query: CatalogQuery,
        _deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError> {
        Err(LinkError::Rejected(CATALOGUE_UNSUPPORTED.to_owned()))
    }

    /// Sends one frame of a package transfer to `host`'s live session, which must still be
    /// `expected_session`.
    ///
    /// `Begin` and `Chunk` return `Ok(None)` once the encoded frame is in the session's
    /// outgoing queue, waiting for room until `deadline`. `End` is queued and then waits for
    /// the agent's answer to that frame: `Ok(Some(..))` is the agent's verdict. A refusal is
    /// [`LinkError::Rejected`]; no answer, or a session that dropped while waiting, is
    /// [`LinkError::Indeterminate`].
    async fn send_transfer(
        &self,
        _host: HostId,
        _frame: TransferFrame,
        _expected_session: AgentSessionId,
        _deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        Err(LinkError::Transport(NOT_SUPPORTED.to_owned()))
    }

    /// Hands one package capability request to `host`'s live session, which must still be
    /// `expected_session` with the journal `expected_epoch` names, and returns once the frame
    /// is in that session's outgoing queue.
    ///
    /// Failing means nothing was queued. Every later failure belongs to
    /// [`AgentLink::capability_result`].
    async fn start_capability(
        &self,
        _host: HostId,
        _req: PackageDispatch,
        _expected_session: AgentSessionId,
        _expected_epoch: JournalEpoch,
        _deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        Err(LinkError::Transport(NOT_SUPPORTED.to_owned()))
    }

    /// Waits for the terminal answer to a request handed over by
    /// [`AgentLink::start_capability`].
    ///
    /// [`LinkError::Rejected`] is the agent's confirmed refusal or failure. Every other error
    /// leaves the outcome unknown.
    async fn capability_result(
        &self,
        _host: HostId,
        _handoff: CapabilityHandoff,
        _deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        Err(LinkError::Transport(NOT_SUPPORTED.to_owned()))
    }
}

/// What a link answers to a package operation it does not implement.
const NOT_SUPPORTED: &str = "not supported by this link";

/// The refusal a link answers when it, or the agent behind it, cannot page a catalogue.
///
/// The registry reads exactly this text as [`CatalogueGap::Unsupported`] rather than a failed
/// page, so an agent too old to answer is reported as not reporting a list at all.
///
/// [`CatalogueGap::Unsupported`]: crate::capability::CatalogueGap
pub const CATALOGUE_UNSUPPORTED: &str = "the agent does not report its capability catalogue";

/// Why signals were not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngestError {
    /// The batch was refused, for example for an unknown host.
    #[error("rejected: {0}")]
    Rejected(String),
    /// The hub cannot accept signals right now; the caller should retry or spool.
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// Where agents, webhooks and probes hand signals to the hub.
#[async_trait]
pub trait SignalSink: Send + Sync {
    /// Accepts a batch, in order. Either the whole batch is accepted or none of it.
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError>;
}

#[cfg(test)]
mod tests;
