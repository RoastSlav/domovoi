//! Who is connected, what is waiting for an answer, and when each host was last heard.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use domovoi_core::ports::{LinkError, Presence};
use domovoi_core::{HostId, RequestId, SkillSummary};
use domovoi_proto::messages::{CheckInfo, SessionId};
use domovoi_proto::{Message, encode};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Agents with a live session right now.
pub const AGENTS_CONNECTED: &str = "domovoi_agents_connected";

/// The most requests one agent may have in flight.
pub const MAX_PENDING: usize = 256;

/// The liveness signal raised for a silent host, until it is heard from again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Liveness {
    /// The host answers on the network but its agent does not.
    AgentDown,
    /// Neither the agent nor the host answers.
    HostDown,
}

/// A live, authenticated session.
pub(crate) struct Live {
    pub session: SessionId,
    pub since: Timestamp,
    pub version: String,
    pub skills: Vec<SkillSummary>,
    pub checks: Vec<CheckInfo>,
    pub outbox: mpsc::Sender<String>,
    pub pending: HashMap<RequestId, oneshot::Sender<Message>>,
    pub cancel: CancellationToken,
}

/// What the hub knows about one host's agent.
#[derive(Default)]
struct Entry {
    live: Option<Live>,
    last_seen: Option<Timestamp>,
    remote_ip: Option<IpAddr>,
    raised: Option<Liveness>,
}

/// One host as the presence check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Watch {
    pub host: HostId,
    pub last_seen: Option<Timestamp>,
    pub remote_ip: Option<IpAddr>,
    pub raised: Option<Liveness>,
}

/// Sessions by host. A host has at most one live session.
#[derive(Default)]
pub(crate) struct Registry {
    hosts: Mutex<BTreeMap<HostId, Entry>>,
}

impl Registry {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<HostId, Entry>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
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
    pub fn register(&self, host: HostId, live: Live, remote_ip: Option<IpAddr>) {
        let mut hosts = self.lock();
        let entry = hosts.entry(host).or_default();
        if let Some(old) = entry.live.take() {
            old.cancel.cancel();
        }
        entry.last_seen = Some(live.since);
        if remote_ip.is_some() {
            entry.remote_ip = remote_ip;
        }
        entry.live = Some(live);
        Self::report(&hosts);
    }

    /// Ends `session` if it is still the host's session. Waiting requests fail.
    pub fn end(&self, host: HostId, session: SessionId) {
        let mut hosts = self.lock();
        if let Some(entry) = hosts.get_mut(&host)
            && entry.live.as_ref().is_some_and(|l| l.session == session)
        {
            entry.live = None;
        }
        Self::report(&hosts);
    }

    /// Closes the host's live session, if any.
    pub fn close(&self, host: HostId) {
        let mut hosts = self.lock();
        if let Some(live) = hosts.get_mut(&host).and_then(|e| e.live.take()) {
            live.cancel.cancel();
        }
        Self::report(&hosts);
    }

    /// Records a frame from the host at `at`.
    pub fn seen(&self, host: HostId, at: Timestamp) {
        if let Some(entry) = self.lock().get_mut(&host) {
            entry.last_seen = Some(at);
        }
    }

    /// Queues `message` for the host's agent and returns the request id and where its
    /// answer will arrive.
    pub fn dispatch(
        &self,
        host: HostId,
        message: &Message,
    ) -> Result<(RequestId, oneshot::Receiver<Message>), LinkError> {
        let mut hosts = self.lock();
        let live = hosts
            .get_mut(&host)
            .and_then(|e| e.live.as_mut())
            .ok_or(LinkError::NotConnected)?;
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
        Ok((id, rx))
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

    /// The skills the host's agent announced when it connected.
    pub fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.lock()
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .map(|l| l.skills.clone())
            .unwrap_or_default()
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
