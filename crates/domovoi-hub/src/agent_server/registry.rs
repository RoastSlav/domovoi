//! Who is connected, what is waiting for an answer, and when each host was last heard.

use std::collections::BTreeMap;
use std::net::IpAddr;
#[cfg(test)]
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard, PoisonError};

use domovoi_core::ports::{
    AgentSnapshot, HelperState, HostView, HostWords, LinkError, Presence, RunnerState,
};
use domovoi_core::{AgentId, AgentSessionId, HostId, JournalEpoch, RequestId, SkillSummary};
use domovoi_proto::messages::{
    AgentStatus, CheckInfo, DockerReport, HelperStatus, HostFacts, SessionId, SkillRunnerStatus,
};
use domovoi_proto::{Message, encode};
use jiff::Timestamp;
#[cfg(test)]
use tokio::sync::Semaphore;
use tokio::sync::{mpsc, oneshot};

use super::registry_state::{RegisterError, State};
use tokio_util::sync::CancellationToken;

/// Agents with a live session right now.
pub const AGENTS_CONNECTED: &str = "domovoi_agents_connected";

/// The most requests one agent may have in flight.
pub const MAX_PENDING: usize = 256;

pub(crate) use super::live::{Live, Liveness, SessionFacts, SessionState, Watch};

#[path = "registry_reserved.rs"]
mod reserved;

/// What the hub knows about one host's agent.
#[derive(Default)]
pub(super) struct Entry {
    pub(super) live: Option<Live>,
    last_seen: Option<Timestamp>,
    /// When this hub session last saw a live session end, if it ever had one.
    absent_since: Option<Timestamp>,
    remote_ip: Option<IpAddr>,
    raised: Option<Liveness>,
}

/// Sessions by host. A host has at most one live session.
#[derive(Default)]
pub(crate) struct Registry {
    state: Mutex<State>,
}

impl Registry {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publishes how many hosts have a live session, from inside the lock.
    fn report(hosts: &BTreeMap<HostId, Entry>) {
        let live = hosts.values().filter(|e| e.live.is_some()).count();
        let live = u32::try_from(live).map_or(f64::from(u32::MAX), f64::from);
        metrics::gauge!(AGENTS_CONNECTED).set(live);
    }

    /// Starts watching `host` without a session, as if last heard at `at`.
    pub fn track(&self, host: HostId, at: Timestamp) {
        let mut hosts = self.lock();
        let entry = hosts.entry(host).or_default();
        entry.last_seen.get_or_insert(at);
    }

    /// Makes `live` the host's session, closing the one it replaces.
    ///
    /// A retired `AgentId` is refused. Its handshake may have finished after the agent was
    /// revoked or replaced, and the registry is the last place that can stop it.
    pub fn register(
        &self,
        host: HostId,
        live: Live,
        remote_ip: Option<IpAddr>,
    ) -> Result<(), RegisterError> {
        let mut hosts = self.lock();
        if hosts.retired.contains(&live.agent) {
            return Err(RegisterError::Retired(live.agent));
        }
        let entry = hosts.entry(host).or_default();
        if let Some(old) = entry.live.take() {
            old.cancel.cancel();
        }
        entry.last_seen = Some(live.since);
        entry.absent_since = None;
        if remote_ip.is_some() {
            entry.remote_ip = remote_ip;
        }
        entry.live = Some(live);
        Self::report(&hosts);
        Ok(())
    }

    /// Retires `agent` for good: records the identity, closes its live session if one is
    /// registered, and makes every later registration of it fail.
    ///
    /// This is the one retirement operation every enrollment path uses, so a handshake that
    /// is still in flight when the agent is revoked or replaced cannot become live later.
    pub fn retire(&self, agent: AgentId, at: Timestamp) {
        let mut state = self.lock();
        state.retired.insert(agent);
        let hosts: Vec<HostId> = state
            .iter()
            .filter(|(_, entry)| entry.live.as_ref().is_some_and(|live| live.agent == agent))
            .map(|(host, _)| *host)
            .collect();
        for host in hosts {
            let Some(entry) = state.get_mut(&host) else {
                continue;
            };
            if let Some(live) = entry.live.take() {
                live.cancel.cancel();
                entry.absent_since = Some(at);
            }
        }
        Self::report(&state);
    }

    /// Whether `agent` has been retired. Test support, for the local enrollment path.
    #[cfg(test)]
    pub fn is_retired(&self, agent: AgentId) -> bool {
        self.lock().retired.contains(&agent)
    }

    /// Test support: holds every registration until a permit is added to `gate`, so a test
    /// can retire an agent between its successful handshake and its registration.
    #[cfg(test)]
    pub fn hold_registrations(&self, gate: Arc<Semaphore>) {
        self.lock().registration_gate = Some(gate);
    }

    /// Test support: the gate registrations wait on, if a test installed one.
    #[cfg(test)]
    pub fn registration_gate(&self) -> Option<Arc<Semaphore>> {
        self.lock().registration_gate.clone()
    }

    /// Ends `session` if it is still the host's session, at `at`. Waiting requests fail and
    /// the session's reconciliation task is cancelled.
    pub fn end(&self, host: HostId, session: SessionId, at: Timestamp) {
        let mut hosts = self.lock();
        if let Some(entry) = hosts.get_mut(&host)
            && entry.live.as_ref().is_some_and(|l| l.session == session)
        {
            if let Some(live) = entry.live.take() {
                // The session that owned the reconciliation task has ended. Only this
                // session's token is cancelled: a replacement's token belongs to it.
                live.cancel.cancel();
            }
            entry.absent_since = Some(at);
        }
        Self::report(&hosts);
    }

    /// Closes the host's live session, if any, at `at`.
    pub fn close(&self, host: HostId, at: Timestamp) {
        let mut hosts = self.lock();
        let mut closed = false;
        if let Some(entry) = hosts.get_mut(&host)
            && let Some(live) = entry.live.take()
        {
            live.cancel.cancel();
            entry.absent_since = Some(at);
            closed = true;
        }
        if closed {
            Self::report(&hosts);
        }
    }

    /// The latest runtime status for the host's live session.
    pub fn agent_status(&self, host: HostId) -> Option<AgentStatus> {
        self.lock()
            .get(&host)
            .and_then(|entry| entry.live.as_ref())
            .and_then(|live| live.agent_status.clone())
    }

    /// Records the agent's latest runtime status for `session`.
    pub fn set_agent_status(&self, host: HostId, session: SessionId, status: AgentStatus) {
        if let Some(live) = self
            .lock()
            .get_mut(&host)
            .and_then(|entry| entry.live.as_mut())
            .filter(|live| live.session == session)
        {
            live.in_container = status.in_container;
            live.agent_status = Some(status);
        }
    }

    /// Records why the agent refused the hub's check overrides for `session`.
    pub fn set_override_refusals(&self, host: HostId, session: SessionId, refused: Vec<String>) {
        if let Some(live) = self
            .lock()
            .get_mut(&host)
            .and_then(|entry| entry.live.as_mut())
            .filter(|live| live.session == session)
        {
            live.override_refusals = refused;
        }
    }

    /// What the live agent refused of the hub's check overrides; empty offline.
    pub fn override_refusals(&self, host: HostId) -> Vec<String> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.override_refusals.clone())
            .unwrap_or_default()
    }

    /// Records a frame from the host at `at`.
    pub fn seen(&self, host: HostId, at: Timestamp) {
        if let Some(entry) = self.lock().get_mut(&host) {
            entry.last_seen = Some(at);
        }
    }

    /// Queues `message` for the host's agent and returns the request id, where its answer
    /// will arrive, and what the accepting session promised about itself.
    ///
    /// With `expected_session` and `expected_epoch`, the request is only queued when the
    /// live session is exactly that connection and reported that journal: the checks and the
    /// queueing are one step under the same lock, so a replacement session cannot receive a
    /// request meant for the old one, even when the journal epoch survived the reconnection.
    pub fn dispatch(
        &self,
        host: HostId,
        message: &Message,
        expected_session: Option<AgentSessionId>,
        expected_epoch: Option<JournalEpoch>,
    ) -> Result<(RequestId, oneshot::Receiver<Message>, SessionFacts), LinkError> {
        let mut hosts = self.lock();
        let live = hosts
            .get_mut(&host)
            .and_then(|e| e.live.as_mut())
            .ok_or(LinkError::NotConnected)?;
        if expected_session.is_some_and(|expected| live.identity != expected) {
            return Err(LinkError::SessionChanged);
        }
        if expected_epoch.is_some_and(|expected| live.journal_epoch != expected.as_uuid()) {
            return Err(LinkError::SessionChanged);
        }
        let facts = SessionFacts {
            authoritative_absence: live.authoritative_absence,
        };
        if live.pending.len() >= MAX_PENDING {
            return Err(busy());
        }
        let id = RequestId::new();
        let frame = encode(message, id, None).map_err(|e| LinkError::Transport(e.to_string()))?;
        match live.outbox.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(busy()),
            Err(mpsc::error::TrySendError::Closed(_)) => return Err(LinkError::NotConnected),
        }
        let (tx, rx) = oneshot::channel();
        live.pending.insert(id, tx);
        Ok((id, rx, facts))
    }

    /// How many requests the live session of `host` is still awaiting answers for.
    #[cfg(test)]
    pub fn pending_len(&self, host: HostId) -> usize {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map_or(0, |live| live.pending.len())
    }

    /// Stops waiting for the answer to `id`.
    pub fn forget(&self, host: HostId, id: RequestId) {
        if let Some(live) = self.lock().get_mut(&host).and_then(|e| e.live.as_mut()) {
            live.pending.remove(&id);
        }
    }

    /// Hands an answer to whoever waits for `reply_to` on `session`. False when nobody does.
    pub fn deliver(
        &self,
        host: HostId,
        session: SessionId,
        reply_to: RequestId,
        message: Message,
    ) -> bool {
        let waiting = self
            .lock()
            .get_mut(&host)
            .and_then(|e| e.live.as_mut())
            .filter(|l| l.session == session)
            .and_then(|l| l.pending.remove(&reply_to));
        waiting.is_some_and(|tx| tx.send(message).is_ok())
    }

    /// What this hub run knows about the host's session: live, ended, or never started.
    /// Unlike [`Registry::presence`] it never looks at `last_seen`, which tracking and
    /// every frame also set.
    pub fn session_state(&self, host: HostId) -> SessionState {
        match self.lock().get(&host) {
            Some(Entry {
                live: Some(live), ..
            }) => SessionState::Live { since: live.since },
            Some(Entry {
                absent_since: Some(at),
                ..
            }) => SessionState::Lost { at: *at },
            _ => SessionState::Never,
        }
    }

    /// Whether the host has a live session, and since when.
    pub fn presence(&self, host: HostId) -> Presence {
        let hosts = self.lock();
        match hosts.get(&host) {
            Some(Entry {
                live: Some(live), ..
            }) => Presence::Connected {
                since: live.since,
                version: live.version.clone(),
            },
            Some(entry) => Presence::Disconnected {
                last_seen: entry.last_seen,
            },
            None => Presence::Disconnected { last_seen: None },
        }
    }

    /// The checks the host's agent reported when it connected; `None` while offline.
    pub fn checks(&self, host: HostId) -> Option<Vec<CheckInfo>> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.checks.clone())
    }

    /// What the host's agent said about its machine when it connected; `None` while offline.
    pub fn facts(&self, host: HostId) -> Option<HostFacts> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.facts.clone())
    }

    /// What the host's agent reported about its container runtime when it connected.
    /// `None` while offline; `Some(None)` from an agent too old to report it.
    pub fn docker(&self, host: HostId) -> Option<Option<DockerReport>> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.docker.clone())
    }

    /// Sends `message` to the host's agent without waiting for an answer. False when the
    /// agent is offline or its queue is full.
    pub fn push(&self, host: HostId, message: &Message) -> bool {
        let hosts = self.lock();
        let Some(live) = hosts.get(&host).and_then(|e| e.live.as_ref()) else {
            return false;
        };
        encode(message, RequestId::new(), None)
            .is_ok_and(|frame| live.outbox.try_send(frame).is_ok())
    }

    /// The journal epoch the host's live session announced, or `None` when it has none.
    pub fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|live| JournalEpoch::from_uuid(live.journal_epoch))
    }

    /// The catalogue hash the host's live session announced, or `None` when it has none.
    pub fn catalog_hash(&self, host: HostId) -> Option<String> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .and_then(|live| live.catalog_hash.clone())
    }

    /// The words and skills the host's live session announced, read together under one lock,
    /// or `None` when it has none.
    pub fn host_view(&self, host: HostId) -> Option<HostView> {
        let hosts = self.lock();
        let live = hosts.get(&host)?.live.as_ref()?;
        Some(HostView {
            words: HostWords {
                runs_as_root: live.runs_as_root,
                in_container: live.in_container,
                runner: match live.skill_runner_status {
                    SkillRunnerStatus::Absent => RunnerState::Absent,
                    SkillRunnerStatus::Unsupported { .. } => RunnerState::Unsupported,
                    SkillRunnerStatus::Ok { .. } => RunnerState::Ready,
                    SkillRunnerStatus::Error { .. } => RunnerState::Failed,
                },
                helper: match live.helper_status {
                    HelperStatus::Absent => HelperState::Absent,
                    HelperStatus::Ok { .. } => HelperState::Ready,
                    HelperStatus::Error { .. } => HelperState::Failed,
                },
                helper_ops: match &live.helper_status {
                    HelperStatus::Ok { ops } => ops.clone(),
                    _ => Vec::new(),
                },
                ceiling_summary: live.ceiling_summary.clone(),
                ceiling_hash: live.ceiling_hash.clone(),
                container_runtime: live.docker.as_ref().is_some_and(|report| report.available),
            },
            skills: live.skills.clone(),
        })
    }

    /// Every host with a live session right now.
    pub fn live_hosts(&self) -> Vec<HostId> {
        self.lock()
            .iter()
            .filter(|(_, entry)| entry.live.is_some())
            .map(|(host, _)| *host)
            .collect()
    }

    /// The cancellation token of the host's live session, if it has one. A reconciliation
    /// task started for an existing session must be tied to it, so the session's end
    /// cancels the task.
    pub fn live_cancel(&self, host: HostId) -> Option<CancellationToken> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|live| live.cancel.clone())
    }

    /// The skills the host's agent announced when it connected.
    pub fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.skills.clone())
            .unwrap_or_default()
    }

    /// The package skills the host's agent announced, on or off.
    pub fn package_skills(&self, host: HostId) -> Vec<domovoi_proto::messages::PackageSkillState> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.package_skills.clone())
            .unwrap_or_default()
    }

    /// The coherent snapshot of the host's live session.
    pub fn snapshot(&self, host: HostId) -> Option<AgentSnapshot> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|live| AgentSnapshot {
                session: live.identity,
                agent: live.agent,
                journal_epoch: JournalEpoch::from_uuid(live.journal_epoch),
                skills: live.skills.clone(),
            })
    }

    /// Every watched host.
    pub fn watches(&self) -> Vec<Watch> {
        self.lock()
            .iter()
            .map(|(host, e)| Watch {
                host: *host,
                last_seen: e.last_seen,
                remote_ip: e.remote_ip,
                raised: e.raised,
            })
            .collect()
    }

    /// Records which liveness signal is raised for the host.
    pub fn set_raised(&self, host: HostId, raised: Option<Liveness>) {
        if let Some(entry) = self.lock().get_mut(&host) {
            entry.raised = raised;
        }
    }
}

fn busy() -> LinkError {
    LinkError::Rejected(format!(
        "the agent already has {MAX_PENDING} requests in flight"
    ))
}
