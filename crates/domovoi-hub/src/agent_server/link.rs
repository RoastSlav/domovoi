//! [`WsAgentLink`]: the engine's way to reach agents through their live sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domovoi_core::attest::Nonce32;
use domovoi_core::capability::{CatalogPageData, CatalogQuery};
use domovoi_core::ports::{
    ActionAnswers, AgentLink, AttestationReply, CATALOGUE_UNSUPPORTED, CheckRecheck, Clock,
    HelperReport, HostAvailability, HostRepo, LinkError, Presence, SkillDispatch, SkillHandoff,
    SkillOutput, ToolOutput,
};
use domovoi_core::{
    AgentSessionId, CheckKey, ExecutionId, HostId, JournalEpoch, ReadToolCall, RequestId,
    SkillSummary, TargetKey,
};
use domovoi_core::{
    CapabilityHandoff, PackageDispatch, PackageOutputs, TransferAnswer, TransferFrame,
};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    ActionStatusQuery, GetCatalog, RunCheck, RunSkill, RunTool, SkillOutcome, ToolFailureCode,
    ToolOutcome,
};
use jiff::Timestamp;
use tokio::sync::oneshot;

use super::compat::{ProtocolState, protocol_state};
pub(super) use super::link_answers::unexpected;
use super::link_answers::{MAX_CATALOG_PAGE, answers_from, implementation_of};
use super::registry::{Registry, SessionFacts, SessionState};

/// Routes requests to the host's current session. Fails fast with
/// [`LinkError::NotConnected`] when the host has none, and with [`LinkError::Timeout`]
/// once the deadline passes.
pub struct WsAgentLink {
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
    hosts: Arc<dyn HostRepo>,
    /// Answers still awaited for requests already queued, by host and request id. Taking
    /// one out is how [`AgentLink::skill_result`] picks up where
    /// [`AgentLink::start_skill`] left off.
    sends: Mutex<HashMap<(HostId, RequestId), oneshot::Receiver<Message>>>,
}

impl WsAgentLink {
    pub(crate) fn new(
        registry: Arc<Registry>,
        clock: Arc<dyn Clock>,
        hosts: Arc<dyn HostRepo>,
    ) -> Self {
        Self {
            registry,
            clock,
            hosts,
            sends: Mutex::new(HashMap::new()),
        }
    }

    /// Locks the awaited answers without panicking on a poisoned lock.
    fn sends(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<(HostId, RequestId), oneshot::Receiver<Message>>> {
        self.sends.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Why a request found no live session: the hub rejected this agent's protocol, or it
    /// is simply not connected. A prediction that it would be incompatible is not a
    /// finding, so that case stays [`LinkError::NotConnected`].
    ///
    /// The lookups read the store, which has no deadline of its own, so they are raced against
    /// `deadline`: a request that failed before anything was queued answers
    /// [`LinkError::Timeout`] when the deadline passes first. Nothing is registered at this
    /// point, so abandoning the lookups leaves no state behind.
    async fn not_connected(&self, host: HostId, deadline: Timestamp) -> LinkError {
        tokio::select! {
            found = self.classify_missing_session(host) => found,
            () = self.clock.sleep_until(deadline) => LinkError::Timeout,
        }
    }

    pub(super) async fn request(
        &self,
        host: HostId,
        message: &Message,
        deadline: Timestamp,
    ) -> Result<Message, LinkError> {
        let (reply, _facts) = self.request_on(host, message, deadline, None).await?;
        Ok(reply)
    }

    /// One hub request, optionally only to a session that still has `expected`, with what
    /// the session that accepted it promised about itself.
    async fn request_on(
        &self,
        host: HostId,
        message: &Message,
        deadline: Timestamp,
        expected: Option<JournalEpoch>,
    ) -> Result<(Message, SessionFacts), LinkError> {
        if self.clock.now() >= deadline {
            return Err(LinkError::Timeout);
        }
        let (id, answer, facts) = match self.registry.dispatch(host, message, None, expected) {
            Ok(parts) => parts,
            Err(LinkError::NotConnected) => return Err(self.not_connected(host, deadline).await),
            Err(err) => return Err(err),
        };
        // The guard forgets the request however this future ends, including being dropped
        // by a caller that gave up, so a registered request is never left behind.
        let _pending = PendingRequest {
            registry: &self.registry,
            host,
            id,
        };
        let reply = tokio::select! {
            reply = answer => reply.map_err(|_| {
                LinkError::Transport("the agent disconnected before answering".to_owned())
            }),
            () = self.clock.sleep_until(deadline) => Err(LinkError::Timeout),
        };
        Ok((reply?, facts))
    }
}

/// Forgets a registered request when dropped, so every exit path, a dropped future included,
/// releases it.
struct PendingRequest<'a> {
    registry: &'a Registry,
    host: HostId,
    id: RequestId,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        self.registry.forget(self.host, self.id);
    }
}

#[async_trait]
impl AgentLink for WsAgentLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        let message = Message::RunTool(RunTool { call, deadline });
        match self.request(host, &message, deadline).await? {
            Message::ToolResult(result) => match result.outcome {
                ToolOutcome::Ok(out) => Ok(out.into()),
                ToolOutcome::Err(f) if f.code == ToolFailureCode::Timeout => {
                    Err(LinkError::Timeout)
                }
                ToolOutcome::Err(f) => Err(LinkError::Rejected(failure_text(
                    &format!("{:?}", f.code),
                    &f.message,
                ))),
            },
            other => Err(unexpected(&other)),
        }
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        let message = Message::RunCheck(RunCheck {
            check,
            target: target.clone(),
            deadline,
        });
        match self.request(host, &message, deadline).await? {
            Message::CheckResult(result) => {
                let mut observations = result.observations;
                if observations.is_empty() {
                    return Err(LinkError::Rejected(
                        "the check reported no observation".to_owned(),
                    ));
                }
                let at = observations
                    .iter()
                    .position(|o| o.target == target)
                    .unwrap_or(0);
                Ok(observations.swap_remove(at).into())
            }
            other => Err(unexpected(&other)),
        }
    }

    async fn start_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError> {
        if self.clock.now() >= deadline {
            return Err(LinkError::Timeout);
        }
        let message = Message::RunSkill(RunSkill::from_dispatch(&req, deadline));
        let (id, answer, _facts) = match self.registry.dispatch(
            host,
            &message,
            Some(expected_session),
            Some(expected_epoch),
        ) {
            Ok(parts) => parts,
            Err(LinkError::NotConnected) => return Err(self.not_connected(host, deadline).await),
            Err(err) => return Err(err),
        };
        self.sends().insert((host, id), answer);
        Ok(SkillHandoff::new(id))
    }

    async fn skill_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        let id = handoff.request();
        let Some(answer) = self.sends().remove(&(host, id)) else {
            return Err(LinkError::Indeterminate(
                "the request was no longer awaited".to_owned(),
            ));
        };
        let _pending = PendingRequest {
            registry: &self.registry,
            host,
            id,
        };
        let reply = tokio::select! {
            reply = answer => reply.map_err(|_| {
                LinkError::Indeterminate(
                    "the agent disconnected before answering".to_owned(),
                )
            }),
            () = self.clock.sleep_until(deadline) => Err(LinkError::Timeout),
        };
        match reply? {
            Message::SkillResult(result) => match result.outcome {
                SkillOutcome::Ok(out) => Ok(out.into()),
                // A terminal answer from the agent: it refused the run or reported that the
                // run failed. The code and the message are both kept.
                SkillOutcome::Err(f) => Err(LinkError::Rejected(failure_text(
                    &format!("{:?}", f.code),
                    &f.message,
                ))),
            },
            Message::Refused(refused) => Err(LinkError::Rejected(format!(
                "{}: {}",
                refused.code.raw(),
                refused.detail
            ))),
            // Anything else is not a result: the run may have happened, so the caller must
            // not read this as a failure.
            other => Err(LinkError::Indeterminate(format!(
                "the agent answered with {}",
                other.kind()
            ))),
        }
    }

    fn presence(&self, host: HostId) -> Presence {
        self.registry.presence(host)
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        // A live session is the answer whether or not an older report says otherwise.
        let state = self.registry.session_state(host);
        if let SessionState::Live { since } = state {
            return HostAvailability::Known { since };
        }
        // A host this hub rejected is reported as incompatible even after its session
        // ended, so an approval waits for an upgrade rather than for a reconnect.
        if let Some(reported) = self.rejected_protocol(host).await {
            return HostAvailability::Incompatible { reported };
        }
        match state {
            SessionState::Lost { at } => HostAvailability::Absent { since: at },
            // A live session was answered above; a host never seen in this hub run is
            // unknown until it connects.
            SessionState::Live { .. } | SessionState::Never => HostAvailability::Unknown,
        }
    }

    fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch> {
        self.registry.journal_epoch(host)
    }

    fn snapshot(&self, host: HostId) -> Option<domovoi_core::ports::AgentSnapshot> {
        self.registry.snapshot(host)
    }

    async fn action_status(
        &self,
        host: HostId,
        ids: Vec<ExecutionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        let query = ActionStatusQuery {
            execution_ids: ids.iter().map(ExecutionId::as_uuid).collect(),
            expected_epoch: expected_epoch.as_uuid(),
        };
        let (reply, facts) = self
            .request_on(
                host,
                &Message::ActionStatus(query),
                deadline,
                Some(expected_epoch),
            )
            .await?;
        match reply {
            Message::ActionStatusAnswers(reply) => {
                Ok(answers_from(reply, facts.authoritative_absence))
            }
            other => Err(LinkError::Indeterminate(format!(
                "the agent answered with {}",
                other.kind()
            ))),
        }
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.registry.skills(host)
    }

    fn catalog_hash(&self, host: HostId) -> Option<String> {
        self.registry.catalog_hash(host)
    }

    fn host_view(&self, host: HostId) -> Option<domovoi_core::ports::HostView> {
        self.registry.host_view(host)
    }

    async fn send_transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        self.transfer(host, frame, expected_session, deadline).await
    }

    async fn start_capability(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.capability_dispatch(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn capability_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        self.capability_answer(host, handoff, deadline).await
    }

    async fn start_unit_call(
        &self,
        host: HostId,
        req: domovoi_core::unit::UnitDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.unit_dispatch(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn unit_call_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<domovoi_core::unit::UnitOutputs, LinkError> {
        self.unit_answer(host, handoff, deadline).await
    }

    async fn start_contain(
        &self,
        host: HostId,
        req: domovoi_core::containment::ContainDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.contain_dispatch(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn contain_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<domovoi_core::containment::ContainOutputs, LinkError> {
        self.contain_answer(host, handoff, deadline).await
    }

    async fn attest_safety(
        &self,
        host: HostId,
        nonce: Nonce32,
        deadline: Timestamp,
    ) -> Result<AttestationReply, LinkError> {
        self.attestation(host, nonce, deadline).await
    }

    fn helper_report(&self, host: HostId) -> Option<HelperReport> {
        self.registry.helper_report(host)
    }

    async fn get_catalog(
        &self,
        host: HostId,
        query: CatalogQuery,
        deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError> {
        // Zero asks for the default page: 100 entries.
        let page_size = if query.page_size == 0 {
            MAX_CATALOG_PAGE
        } else {
            query.page_size.clamp(1, MAX_CATALOG_PAGE)
        };
        let message = Message::GetCatalog(GetCatalog {
            page: query.page,
            page_size,
        });
        match self.request(host, &message, deadline).await? {
            Message::CatalogPage(page) => {
                let mut entries = Vec::with_capacity(page.entries.len());
                for entry in page.entries {
                    entries.push(implementation_of(entry).map_err(|text| {
                        // The page arrived but cannot be read: the walk reports it as a
                        // malformed page rather than a failed one.
                        LinkError::Indeterminate(text)
                    })?);
                }
                Ok(CatalogPageData {
                    entries,
                    next_page: page.next_page,
                })
            }
            Message::Unsupported(_) => Err(LinkError::Rejected(CATALOGUE_UNSUPPORTED.to_owned())),
            Message::Error(err) => Err(LinkError::Rejected(err.message)),
            other => Err(unexpected(&other)),
        }
    }
}
#[path = "link_contain.rs"]
mod contain;
#[path = "link_logs.rs"]
mod logs;
#[path = "link_package.rs"]
mod package;

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;

/// The text of a refused tool or skill: the agent's code and message, except that the agent's
/// internal "skill_runner_unavailable" token says in words what is missing and what to do.
fn failure_text(code: &str, message: &str) -> String {
    match message.strip_prefix("skill_runner_unavailable:") {
        Some(reason) => format!(
            "this host cannot run skills: its agent has no usable skill runner ({}). Install one with install.sh (see the Capabilities page for this host)",
            reason.trim()
        ),
        None => format!("{code}: {message}"),
    }
}

#[cfg(test)]
mod failure_text_tests {
    use super::failure_text;

    #[test]
    fn the_runner_token_is_said_in_words() {
        let text = failure_text("Exec", "skill_runner_unavailable: absent");
        assert!(text.contains("cannot run skills"), "{text}");
        assert!(!text.contains("Exec"), "{text}");
        assert_eq!(failure_text("Exec", "boom"), "Exec: boom");
    }
}
