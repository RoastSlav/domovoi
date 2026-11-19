//! [`WsAgentLink`]: the engine's way to reach agents through their live sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domovoi_core::capability::{
    CatalogImplementation, CatalogPageData, CatalogQuery, ContractVersion,
};
use domovoi_core::ports::{
    ActionAnswers, AgentLink, CATALOGUE_UNSUPPORTED, CheckRecheck, Clock, HostAvailability,
    HostRepo, HostWords, LinkError, LogLine, Presence, SkillDispatch, SkillHandoff, SkillOutput,
    ToolOutput,
};
use domovoi_core::{
    ActionId, AgentSessionId, CheckKey, ContractId, HostId, JournalEpoch, ReadToolCall, RequestId,
    SkillSummary, TargetKey,
};
use domovoi_proto::messages::{
    ActionStatusQuery, CatalogEntry, GetCatalog, GetLogs, RunCheck, RunSkill, RunTool,
    SkillOutcome, ToolFailureCode, ToolOutcome,
};
use domovoi_proto::{Message, PROTOCOL_VERSION};
use jiff::SignedDuration;
use jiff::Timestamp;
use tokio::sync::oneshot;

use super::compat::{ProtocolState, protocol_state};
use super::link_answers::answers_from;
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

    /// The store lookups behind [`Self::not_connected`], without a deadline.
    async fn classify_missing_session(&self, host: HostId) -> LinkError {
        let report = match self.hosts.live_agent(host).await {
            Ok(Some(agent)) => self.hosts.agent_report(agent.agent).await,
            Ok(None) => return LinkError::NotConnected,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return LinkError::NotConnected;
            }
        };
        let report = match report {
            Ok(report) => report,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return LinkError::NotConnected;
            }
        };
        match protocol_state(report.as_ref(), PROTOCOL_VERSION) {
            ProtocolState::Incompatible { .. } => LinkError::Incompatible,
            _ => LinkError::NotConnected,
        }
    }

    /// The version the host's agent reported when this hub rejected it, or `None` when
    /// the agent is not rejected. A report that merely predates this hub's protocol is a
    /// prediction, not a finding, so it is `None` too.
    async fn rejected_protocol(&self, host: HostId) -> Option<String> {
        let agent = match self.hosts.live_agent(host).await {
            Ok(Some(agent)) => agent,
            Ok(None) => return None,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent");
                return None;
            }
        };
        let report = match self.hosts.agent_report(agent.agent).await {
            Ok(report) => report,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return None;
            }
        };
        match protocol_state(report.as_ref(), PROTOCOL_VERSION) {
            ProtocolState::Incompatible { reported, .. } => Some(reported.to_string()),
            _ => None,
        }
    }

    async fn request(
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

    /// The recent log lines of the agent on `host`: waits 10 s for a reply.
    /// The window may not fit one frame, so the agent answers in chunks and the
    /// hub asks again with `offset` advanced, until the reply says it has no
    /// more lines. An agent that predates `get_logs` replies `unsupported`,
    /// reported as [`LinkError::Rejected`] telling the person to update the agent.
    pub async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<LogLine>, LinkError> {
        let deadline = self.clock.now() + SignedDuration::from_secs(10);
        let window = usize::from(lines);
        let mut got: Vec<LogLine> = Vec::new();
        loop {
            let get = GetLogs {
                lines,
                min_level: min_level.clone(),
                // The window is at most `lines`, which fits in a `u16`.
                offset: got.len().min(window) as u16,
                deadline,
            };
            let reply = match self.request(host, &Message::GetLogs(get), deadline).await? {
                Message::Logs(reply) => reply,
                Message::Unsupported(_) => {
                    if !got.is_empty() {
                        return Ok(got);
                    }
                    return Err(LinkError::Rejected(
                        "the agent is too old to send logs; update it".to_owned(),
                    ));
                }
                Message::Error(err) => {
                    // A full request queue answers with a busy error right away, so
                    // the caller does not wait out the deadline for it.
                    return Err(LinkError::Rejected(err.message));
                }
                other => return Err(unexpected(&other)),
            };
            let more = reply.has_more;
            let empty = reply.lines.is_empty();
            got.extend(reply.lines);
            if got.len() >= window || !more || empty {
                break;
            }
        }
        got.truncate(window);
        Ok(got)
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

fn unexpected(message: &Message) -> LinkError {
    LinkError::Rejected(format!("the agent answered with {}", message.kind()))
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
                ToolOutcome::Err(f) => {
                    Err(LinkError::Rejected(format!("{:?}: {}", f.code, f.message)))
                }
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
                SkillOutcome::Err(f) => {
                    Err(LinkError::Rejected(format!("{:?}: {}", f.code, f.message)))
                }
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
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        let query = ActionStatusQuery {
            execution_ids: ids.iter().map(ActionId::as_uuid).collect(),
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

    fn host_words(&self, host: HostId) -> Option<HostWords> {
        self.registry.host_words(host)
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

/// The largest page the wire and the registry ask for.
const MAX_CATALOG_PAGE: u32 = 100;

/// One wire entry as the core type, or why its id or version is unreadable.
///
/// The descriptor is ignored on purpose: it is opaque at the wire layer, and deriving a
/// contract from it would let an agent define its own authority (RW-07).
fn implementation_of(entry: CatalogEntry) -> Result<CatalogImplementation, String> {
    let contract = ContractId::parse(&entry.contract)
        .map_err(|err| format!("{:?}: {}", entry.contract, err))?;
    let version = ContractVersion::new(entry.version)
        .map_err(|err| format!("{}: {}", entry.contract, err))?;
    Ok(CatalogImplementation { contract, version })
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
