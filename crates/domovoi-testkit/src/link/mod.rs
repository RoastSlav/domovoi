//! A fake agent link, and the recording doubles next to it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, AgentLink, AgentSnapshot, CheckRecheck,
    HostAvailability, LinkError, Presence, SkillDispatch, SkillHandoff, SkillOutput, ToolOutput,
};
use domovoi_core::{
    ActionId, AgentId, AgentSessionId, CheckKey, HostId, JournalEpoch, ReadToolCall, SkillSummary,
    TargetKey,
};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use jiff::Timestamp;

mod recorders;
mod request;
mod script;

pub use recorders::{RecordingReconciler, RecordingSignalSink};
pub use request::LinkRequest;

use crate::lock;

type ToolHandler =
    Box<dyn Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync>;
type SkillHandler =
    Box<dyn Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError> + Send + Sync>;
type CheckHandler = Box<
    dyn Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError> + Send + Sync,
>;
type StatusHandler = Arc<
    dyn Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError> + Send + Sync,
>;

#[derive(Default)]
struct Handlers {
    tool: Option<ToolHandler>,
    skill: Option<SkillHandler>,
    check: Option<CheckHandler>,
}

/// An [`AgentLink`] with scripted presence, skills, host facts and handlers.
///
/// Every request is recorded. A host that is not [`Presence::Connected`] gets
/// [`LinkError::NotConnected`]; a connected host with no handler for the request kind gets
/// [`LinkError::Rejected`]. Deadlines are recorded but never enforced.
pub struct FakeAgentLink {
    presence: Mutex<BTreeMap<HostId, Presence>>,
    availability: Mutex<BTreeMap<HostId, HostAvailability>>,
    skills: Mutex<BTreeMap<HostId, Vec<SkillSummary>>>,
    facts: Mutex<BTreeMap<HostId, HostFacts>>,
    checks: Mutex<BTreeMap<HostId, Vec<CheckInfo>>>,
    epochs: Mutex<BTreeMap<HostId, JournalEpoch>>,
    agents: Mutex<BTreeMap<HostId, AgentId>>,
    sessions: Mutex<BTreeMap<HostId, AgentSessionId>>,
    answers: Mutex<BTreeMap<HostId, BTreeMap<ActionId, ActionAnswer>>>,
    status: Mutex<Option<StatusHandler>>,
    /// When set, `start_skill` waits for a permit before it reports the request handed over,
    /// so a test can hold a dispatch at the point where it has been committed but not sent.
    handoff_gate: Mutex<Option<std::sync::Arc<tokio::sync::Semaphore>>>,
    /// The error `start_skill` answers with once, instead of handing the request over.
    handoff_failure: Mutex<Option<LinkError>>,
    /// When set for a host, `action_status` waits for a permit from the gate before it
    /// answers, so a test can hold a pass inside its journal query.
    status_gates: Mutex<BTreeMap<HostId, std::sync::Arc<tokio::sync::Semaphore>>>,
    /// Skill requests already handed over, by request id, waiting for `skill_result`.
    sent_skills: Mutex<BTreeMap<domovoi_core::RequestId, SkillDispatch>>,
    handlers: Mutex<Handlers>,
    requests: Mutex<Vec<LinkRequest>>,
    snapshot_calls: std::sync::atomic::AtomicUsize,
    /// Test support: answers the next `snapshot` calls in order, before the live state.
    scripted_snapshots: Mutex<VecDeque<Option<AgentSnapshot>>>,
}

impl FakeAgentLink {
    /// A link where no host is connected, nothing announced and no handlers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(BTreeMap::new()),
            availability: Mutex::new(BTreeMap::new()),
            skills: Mutex::new(BTreeMap::new()),
            facts: Mutex::new(BTreeMap::new()),
            checks: Mutex::new(BTreeMap::new()),
            epochs: Mutex::new(BTreeMap::new()),
            agents: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            answers: Mutex::new(BTreeMap::new()),
            status: Mutex::new(None),
            handoff_gate: Mutex::new(None),
            handoff_failure: Mutex::new(None),
            status_gates: Mutex::new(BTreeMap::new()),
            sent_skills: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(Handlers::default()),
            requests: Mutex::new(Vec::new()),
            snapshot_calls: std::sync::atomic::AtomicUsize::new(0),
            scripted_snapshots: Mutex::new(VecDeque::new()),
        }
    }

    /// Makes `start_skill` wait for a permit from `gate` before reporting the hand-off, so a
    /// test can hold a dispatch between its commit and its send. Each `add_permits(1)`
    /// releases one hand-off.
    pub fn hold_skill_handoffs(&self, gate: std::sync::Arc<tokio::sync::Semaphore>) {
        *lock(&self.handoff_gate) = Some(gate);
    }

    /// Makes the next `start_skill` fail with `error` instead of handing the request over,
    /// as a full agent queue or an offline host would.
    pub fn fail_next_skill_handoff(&self, error: LinkError) {
        *lock(&self.handoff_failure) = Some(error);
    }

    /// Makes `action_status` for `host` wait for a permit from `gate` before answering, so a
    /// test can hold a reconciliation pass inside its journal query. The request is recorded
    /// before the wait, so the query is visible while it is held.
    pub fn hold_status_queries(&self, host: HostId, gate: std::sync::Arc<tokio::sync::Semaphore>) {
        lock(&self.status_gates).insert(host, gate);
    }

    /// Sets a host's presence.
    pub fn set_presence(&self, host: HostId, presence: Presence) {
        lock(&self.presence).insert(host, presence);
    }

    /// The enrolled agent this fake reports for `host`, creating one on first use.
    pub fn agent_id(&self, host: HostId) -> AgentId {
        *lock(&self.agents).entry(host).or_default()
    }

    /// Names the enrolled agent a host's session belongs to.
    pub fn set_agent(&self, host: HostId, agent: AgentId) {
        lock(&self.agents).insert(host, agent);
    }

    /// The connection identity this fake reports for `host`.
    pub fn session_id(&self, host: HostId) -> AgentSessionId {
        *lock(&self.sessions).entry(host).or_default()
    }

    /// Replaces the authenticated connection for `host`, as a reconnect would.
    pub fn new_session(&self, host: HostId) -> AgentSessionId {
        let session = AgentSessionId::new();
        lock(&self.sessions).insert(host, session);
        session
    }

    /// Sets what `availability` reports for a host, separate from its presence. A test uses
    /// this to say a host is unknown even though the fake knows when it was last seen.
    pub fn set_availability(&self, host: HostId, availability: HostAvailability) {
        lock(&self.availability).insert(host, availability);
    }

    /// Drops a scripted availability, so the host's presence decides again.
    pub fn clear_availability(&self, host: HostId) {
        lock(&self.availability).remove(&host);
    }

    /// Sets the skills a host announces.
    pub fn set_skills(&self, host: HostId, skills: Vec<SkillSummary>) {
        lock(&self.skills).insert(host, skills);
    }

    /// Sets what a host's agent said about its machine, as a handshake would.
    pub fn set_facts(&self, host: HostId, facts: HostFacts) {
        lock(&self.facts).insert(host, facts);
    }

    /// What the host's agent said about its machine, `None` while never announced.
    #[must_use]
    pub fn host_facts(&self, host: HostId) -> Option<HostFacts> {
        lock(&self.facts).get(&host).cloned()
    }

    /// Sets the checks a host's agent announced, as a handshake would.
    pub fn set_checks(&self, host: HostId, checks: Vec<CheckInfo>) {
        lock(&self.checks).insert(host, checks);
    }

    /// The checks the host's agent announced; empty while never announced.
    #[must_use]
    pub fn host_checks(&self, host: HostId) -> Vec<CheckInfo> {
        lock(&self.checks).get(&host).cloned().unwrap_or_default()
    }

    /// Answers `run_tool` with `handler`.
    pub fn on_tool(
        &self,
        handler: impl Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync + 'static,
    ) {
        lock(&self.handlers).tool = Some(Box::new(handler));
    }

    /// Answers `run_skill` with `handler`.
    pub fn on_skill(
        &self,
        handler: impl Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).skill = Some(Box::new(handler));
    }

    /// Answers `run_check` with `handler`.
    pub fn on_check(
        &self,
        handler: impl Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).check = Some(Box::new(handler));
    }

    /// Sets the journal epoch a host's agent reported.
    pub fn set_journal_epoch(&self, host: HostId, epoch: JournalEpoch) {
        lock(&self.epochs).insert(host, epoch);
    }

    /// Drops a host's journal epoch, so `journal_epoch` answers `None` for it.
    pub fn clear_journal_epoch(&self, host: HostId) {
        lock(&self.epochs).remove(&host);
    }

    /// Scripts what the host's journal says about one run.
    pub fn answer(&self, host: HostId, id: ActionId, answer: ActionAnswer) {
        lock(&self.answers)
            .entry(host)
            .or_default()
            .insert(id, answer);
    }

    /// Answers every `action_status` with `handler`, instead of the scripted answers.
    pub fn on_action_status(
        &self,
        handler: impl Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        *lock(&self.status) = Some(Arc::new(handler));
    }

    /// Every request so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LinkRequest> {
        lock(&self.requests).clone()
    }

    fn connected(&self, host: HostId) -> Result<(), LinkError> {
        match self.presence(host) {
            Presence::Connected { .. } => Ok(()),
            Presence::Disconnected { .. } => Err(LinkError::NotConnected),
        }
    }
}

impl Default for FakeAgentLink {
    fn default() -> Self {
        Self::new()
    }
}

fn no_handler() -> LinkError {
    LinkError::Rejected("no handler registered".to_owned())
}

#[async_trait]
impl AgentLink for FakeAgentLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        lock(&self.requests).push(LinkRequest::Tool {
            host,
            call: call.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.tool.as_ref().ok_or_else(no_handler)?;
        handler(host, &call)
    }

    async fn start_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError> {
        self.connected(host)?;
        if let Some(error) = lock(&self.handoff_failure).take() {
            return Err(error);
        }
        let gate = lock(&self.handoff_gate).clone();
        if let Some(gate) = gate {
            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|_| LinkError::NotConnected)?;
        }
        if self.session_id(host) != expected_session
            || self.journal_epoch(host) != Some(expected_epoch)
        {
            return Err(LinkError::SessionChanged);
        }
        lock(&self.requests).push(LinkRequest::Skill {
            host,
            dispatch: req.clone(),
            deadline,
        });
        let id = domovoi_core::RequestId::new();
        lock(&self.sent_skills).insert(id, req);
        Ok(SkillHandoff::new(id))
    }

    async fn skill_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        _deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        let Some(req) = lock(&self.sent_skills).remove(&handoff.request()) else {
            return Err(LinkError::Indeterminate(
                "no skill request was handed over".to_owned(),
            ));
        };
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.skill.as_ref().ok_or_else(no_handler)?;
        handler(host, &req)
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        lock(&self.requests).push(LinkRequest::Check {
            host,
            check: check.clone(),
            target: target.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.check.as_ref().ok_or_else(no_handler)?;
        handler(host, &check, target.as_ref())
    }

    fn presence(&self, host: HostId) -> Presence {
        lock(&self.presence)
            .get(&host)
            .cloned()
            .unwrap_or(Presence::Disconnected { last_seen: None })
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        if let Some(scripted) = lock(&self.availability).get(&host).cloned() {
            return scripted;
        }
        // The trait's default, spelled out because calling it here would recurse.
        match self.presence(host) {
            Presence::Connected { since, .. } => HostAvailability::Known { since },
            Presence::Disconnected {
                last_seen: Some(since),
            } => HostAvailability::Absent { since },
            Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
        }
    }

    fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch> {
        lock(&self.epochs).get(&host).copied()
    }

    fn snapshot(&self, host: HostId) -> Option<AgentSnapshot> {
        self.snapshot_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(scripted) = lock(&self.scripted_snapshots).pop_front() {
            return scripted;
        }
        match self.presence(host) {
            Presence::Connected { .. } => Some(AgentSnapshot {
                session: self.session_id(host),
                agent: self.agent_id(host),
                journal_epoch: self.journal_epoch(host).unwrap_or_default(),
                skills: self.skills(host),
            }),
            Presence::Disconnected { .. } => None,
        }
    }

    async fn action_status(
        &self,
        host: HostId,
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        lock(&self.requests).push(LinkRequest::Status {
            host,
            ids: ids.clone(),
            expected_epoch,
            deadline,
        });
        self.connected(host)?;
        let gate = lock(&self.status_gates).get(&host).cloned();
        if let Some(gate) = gate {
            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|_| LinkError::NotConnected)?;
        }
        if let Some(handler) = lock(&self.status).clone() {
            return handler(host, &ids, expected_epoch);
        }
        let scripted = lock(&self.answers);
        let host_answers = scripted.get(&host);
        let answers = ids
            .into_iter()
            .map(|id| ActionAnswerEntry {
                execution: id,
                answer: host_answers
                    .and_then(|known| known.get(&id))
                    .cloned()
                    .unwrap_or(ActionAnswer::NotFound),
            })
            .collect();
        Ok(ActionAnswers {
            epoch: expected_epoch,
            answers,
        })
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        lock(&self.skills).get(&host).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_agent_link_checks_presence_then_handlers() {
        let link = FakeAgentLink::new();
        let host = HostId::new();
        let at = Timestamp::constant(1_700_000_000, 0);
        let call = ReadToolCall::HostOverview;
        assert_eq!(
            link.run_tool(host, call.clone(), at).await,
            Err(LinkError::NotConnected)
        );

        link.set_presence(
            host,
            Presence::Connected {
                since: at,
                version: "0.1.0".to_owned(),
            },
        );
        assert!(matches!(
            link.run_tool(host, call.clone(), at).await,
            Err(LinkError::Rejected(_))
        ));

        link.on_tool(|_, _| {
            Ok(ToolOutput {
                text: "up 3 days".to_owned(),
                truncated: false,
            })
        });
        let out = link.run_tool(host, call, at).await.unwrap();
        assert_eq!(out.text, "up 3 days");
        assert_eq!(link.requests().len(), 3);
        assert!(link.skills(host).is_empty());
    }
}
