//! The link to the hub: connect, authenticate, exchange messages, reconnect.
//!
//! - [`tls`] pins the hub certificate; [`ws`] frames the protocol over a WebSocket.
//! - [`handshake`](mod@handshake) proves the agent holds its key, bound to the TLS session.
//! - [`Link::run`] reconnects with [`backoff`] until shutdown or revocation. A hub that
//!   refuses this agent's protocol leaves it idle instead: it stops reconnecting, logs
//!   hourly and writes [`status`]. Each connection replays the spool, sends a `ping` every
//!   heartbeat and gives up after three unanswered pings.
//! - Requests (`run_tool`, `run_skill`, `run_check`) are forwarded to the request handler
//!   and never handled on the link task.

pub mod backoff;
pub mod frame;
pub mod handshake;
pub mod report;
pub mod status;
pub mod tls;
pub mod transfers;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use domovoi_core::ports::{Clock, HostFs};
use domovoi_core::{RequestId, SkillSummary};
use domovoi_proto::messages::PackageSkillState;
use domovoi_proto::messages::{
    ActionStatusQuery, AttestSafety, CheckInfo, GetCatalog, GetLogs, Pong, RunCapability, RunCheck,
    RunSkill, RunTool, Signals, SkillsUpdate, Unsupported,
};
use domovoi_proto::{Decoded, Message, Transport};
use jiff::SignedDuration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use self::backoff::{Backoff, STABLE_AFTER};
use self::handshake::{
    AgentStatusSource, Announce, HelloExtensions, STEP_TIMEOUT, handshake, heartbeat,
};
use self::status::LinkStatus;
pub use self::transfers::{IncomingTransfer, TransferFrame};
use self::ws::{Connected, Connector};
use crate::error::LinkError;
use crate::facts::host_facts;
use crate::identity::Identity;
use crate::spool::SharedSpool;
use crate::time::{sleep, within};

/// Pings the hub may leave unanswered before the connection counts as dead.
pub const MISSED_PONGS: u32 = 3;
/// Requests that may wait for the handler before new ones are refused as busy.
pub const REQUEST_QUEUE: usize = 32;
/// Outgoing messages that may wait for the link.
pub const OUTBOX: usize = 256;
/// How often an idle agent says it is still waiting to be upgraded.
pub const IDLE_NOTICE_INTERVAL: SignedDuration = SignedDuration::from_hours(1);
/// How often the agent compares its runtime status and reports a change.
///
/// The probe compares the current snapshot of the runtime status (`runs_as_root`, helper,
/// skill runner and ceiling) and sends a change; it does not refresh the environment.
pub const STATUS_PROBE_INTERVAL: SignedDuration = SignedDuration::from_secs(60);

/// A request from the hub, waiting for the request handler.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestKind {
    /// Run one read tool.
    Tool(RunTool),
    /// Run one approved skill.
    Skill(RunSkill),
    /// Run one check now.
    Check(RunCheck),
    /// Send recent log lines.
    Logs(GetLogs),
    /// Run one capability.
    Capability(RunCapability),
    /// Report what the journal knows about executions.
    ActionStatus(ActionStatusQuery),
    /// One page of the capability catalogue.
    Catalog(GetCatalog),
    /// A challenge for the helper's signed safety report.
    Attest(AttestSafety),
}

/// A request and the id its reply must carry in `reply_to`.
#[derive(Debug, Clone, PartialEq)]
pub struct IncomingRequest {
    /// The request's envelope id.
    pub id: RequestId,
    /// What is asked.
    pub kind: RequestKind,
}

/// A message for the hub.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    /// The message.
    pub message: Message,
    /// The request it answers, if any.
    pub reply_to: Option<RequestId>,
}

/// Sends messages to the hub through the link.
#[derive(Debug, Clone)]
pub struct LinkHandle {
    outbox: mpsc::Sender<Outgoing>,
}

impl LinkHandle {
    /// A handle and the receiving end of its outbox, for a link or a test to drain.
    #[must_use]
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<Outgoing>) {
        let (outbox, rx) = mpsc::channel(capacity);
        (Self { outbox }, rx)
    }

    /// Queues `message` for the hub. Returns `false` when the outbox is full or the link has
    /// stopped; the message is then dropped.
    pub fn send(&self, message: Message, reply_to: Option<RequestId>) -> bool {
        self.outbox.try_send(Outgoing { message, reply_to }).is_ok()
    }
}

/// What the link needs.
pub struct LinkParts {
    /// Opens connections.
    pub connector: Arc<dyn Connector>,
    /// The agent's identity.
    pub identity: Identity,
    /// Time for heartbeats, timeouts and backoff.
    pub clock: Arc<dyn Clock>,
    /// The host's files, for the facts in `hello`.
    pub fs: Arc<dyn HostFs>,
    /// The local catalogue, announced in `hello`.
    pub skills: Vec<SkillSummary>,
    /// Every check with its local settings, announced in `hello`.
    pub checks: Vec<CheckInfo>,
    /// What the agent found out about the container runtime, announced in `hello`.
    pub docker: Option<domovoi_proto::messages::DockerReport>,
    /// The rest of the 1.2 `hello` extensions.
    pub extensions: HelloExtensions,
    /// The runtime status the agent reports in `agent_status`.
    pub status: AgentStatusSource,
    /// Where the hub's check overrides take effect; `None` ignores them.
    pub overrides: Option<crate::overrides::EffectiveChecks>,
    /// Signal batches to send and replay.
    pub spool: Arc<SharedSpool>,
    /// Jitter factors for the reconnect delay.
    pub jitter: Box<dyn FnMut() -> f64 + Send + Sync>,
    /// Where to write `link-status.json`; `None` keeps no status.
    pub status_dir: Option<PathBuf>,
    /// Where `transfer_*` frames go; `None` answers them as not enabled.
    pub transfers: Option<mpsc::Sender<IncomingTransfer>>,
    /// Where enabled package skills and the state of stored ones come from, read at every
    /// connection; `None` announces only `skills`.
    pub package_skills: Option<Arc<crate::requests::RequestCtx>>,
    /// Fires when the package skills changed; the link then sends `skills_update` on the
    /// open session.
    pub resync: Option<Arc<tokio::sync::Notify>>,
}

/// The hub link task.
pub struct Link {
    parts: LinkParts,
    backoff: Backoff,
    /// The outage and session the log lines tell about.
    outage: report::Outage,
    outbox: mpsc::Receiver<Outgoing>,
    requests: mpsc::Sender<IncomingRequest>,
}

/// The skill lists and catalogue hash as they are now, for `hello` and `skills_update`.
struct SkillsNow {
    skills: Vec<SkillSummary>,
    package_states: Vec<PackageSkillState>,
    extensions: HelloExtensions,
}

impl Link {
    /// A link, the handle for sending through it and the stream of hub requests.
    #[must_use]
    pub fn new(mut parts: LinkParts) -> (Self, LinkHandle, mpsc::Receiver<IncomingRequest>) {
        let (handle, outbox) = LinkHandle::channel(OUTBOX);
        let (requests, requests_rx) = mpsc::channel(REQUEST_QUEUE);
        let jitter = std::mem::replace(&mut parts.jitter, Box::new(|| 1.0));
        let link = Self {
            parts,
            backoff: Backoff::new(jitter),
            outage: report::Outage::default(),
            outbox,
            requests,
        };
        (link, handle, requests_rx)
    }

    /// Connects and serves the hub until `shutdown`, or until the hub revokes this agent.
    /// An incompatible protocol leaves it idle until `shutdown`.
    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), LinkError> {
        let clock = self.parts.clock.clone();
        loop {
            let mut unreachable = None;
            let attempt = tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                attempt = self.parts.connector.connect() => attempt,
            };
            match attempt {
                Ok(connected) => {
                    let started = clock.now();
                    let result = self.session(connected, &shutdown).await;
                    self.end_transfers().await;
                    if clock.now().duration_since(started) >= STABLE_AFTER {
                        self.backoff.reset();
                    }
                    match result {
                        Ok(()) => return Ok(()),
                        Err(err) if err.is_idle() => {
                            self.idle(err, &shutdown).await;
                            return Ok(());
                        }
                        Err(LinkError::Resync) => {
                            // Deliberate, and the hub is waiting for the new list: no
                            // backoff, and nothing went wrong to report.
                            tracing::info!("connecting again to announce the new skills");
                            continue;
                        }
                        Err(err) if err.is_final() => {
                            tracing::error!(%err, "the hub link stopped");
                            return Err(err);
                        }
                        Err(err) => {
                            write_status(
                                self.parts.status_dir.as_deref(),
                                &LinkStatus {
                                    state: "reconnecting".to_owned(),
                                    updated_at: clock.now(),
                                    connected_since: None,
                                    hub_protocol: None,
                                    agent_protocol: None,
                                    last_notice_at: None,
                                    detail: Some(err.to_string()),
                                },
                            );
                            self.outage.log_dropped(&err, clock.now());
                        }
                    }
                }
                Err(err) if err.is_idle() => {
                    self.idle(err, &shutdown).await;
                    return Ok(());
                }
                Err(err) if err.is_final() => return Err(err),
                Err(err) => unreachable = Some(err),
            }
            let delay = self.backoff.next_delay();
            if let Some(err) = unreachable {
                let hub = self.parts.connector.target();
                self.outage.log_unreachable(&hub, &err, clock.now(), delay);
            }
            tracing::debug!(delay_ms = delay.as_millis(), "reconnecting");
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = sleep(&*clock, delay) => {}
            }
        }
    }

    fn skills_now(&self) -> SkillsNow {
        let package_view = self
            .parts
            .package_skills
            .as_ref()
            .map(|ctx| ctx.package_skills());
        let mut skills = self.parts.skills.clone();
        let mut package_states = Vec::new();
        let mut extensions = self.parts.extensions.clone();
        if let (Some(ctx), Some(view)) = (&self.parts.package_skills, package_view) {
            // The announced hash must describe the list `get_catalog` pages now: a hash kept
            // from start-up disagrees with it as soon as a package is enabled or disabled.
            let enabled: Vec<_> = view
                .runnable
                .iter()
                .map(|r| r.skill.manifest().name.clone())
                .collect();
            extensions.catalog_hash =
                handshake::catalog_announcement(&crate::requests::catalog_entries_with(
                    &ctx.catalog,
                    ctx.package_skills_surface(),
                    &enabled,
                ));
            for runnable in &view.runnable {
                let mut found = runnable.catalog.summaries();
                ctx.mark_hidden(&runnable.catalog, &mut found);
                skills.extend(found);
            }
            package_states = view.states;
        }
        SkillsNow {
            skills,
            package_states,
            extensions,
        }
    }

    /// Stays up without reconnecting until shutdown, saying hourly that an upgrade is the
    /// only way out.
    async fn idle(&mut self, err: LinkError, shutdown: &CancellationToken) {
        let clock = self.parts.clock.clone();
        let status_dir = self.parts.status_dir.clone();
        let (hub_protocol, agent_protocol) = match &err {
            LinkError::Incompatible { hub, agent } => {
                (Some(hub.to_string()), Some(agent.to_string()))
            }
            _ => (None, None),
        };
        let detail =
            format!("{err}; upgrade the agent and restart it; staying up without reconnecting");
        let now = clock.now();
        let mut status = LinkStatus {
            state: "incompatible".to_owned(),
            updated_at: now,
            connected_since: None,
            hub_protocol,
            agent_protocol,
            last_notice_at: Some(now),
            detail: Some(detail.clone()),
        };
        write_status(status_dir.as_deref(), &status);
        tracing::error!("{detail}");
        loop {
            let now = clock.now();
            let next = now.saturating_add(IDLE_NOTICE_INTERVAL).unwrap_or(now);
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = clock.sleep_until(next) => {
                    let now = clock.now();
                    status.updated_at = now;
                    status.last_notice_at = Some(now);
                    write_status(status_dir.as_deref(), &status);
                    tracing::error!("{detail}");
                }
                Some(_) = self.outbox.recv() => {}
            }
        }
    }

    async fn session(
        &mut self,
        connected: Connected,
        shutdown: &CancellationToken,
    ) -> Result<(), LinkError> {
        let clock = self.parts.clock.clone();
        let Connected {
            mut transport,
            session_hint,
        } = connected;
        let SkillsNow {
            skills,
            package_states,
            extensions,
        } = self.skills_now();
        let announce = Announce {
            facts: host_facts(&*self.parts.fs).await,
            skills: &skills,
            package_skills: &package_states,
            checks: &self.parts.checks,
            docker: self.parts.docker.as_ref(),
            extensions: &extensions,
        };
        let shake = handshake(
            transport.as_mut(),
            &self.parts.identity,
            &session_hint,
            announce,
        );
        let welcome = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            welcome = within(&*clock, STEP_TIMEOUT, shake) => welcome.ok_or(LinkError::Timeout)??,
        };
        let now = clock.now();
        let hub = self.parts.connector.target();
        self.outage.log_connected(&hub, &welcome, now);
        write_status(
            self.parts.status_dir.as_deref(),
            &LinkStatus {
                state: "connected".to_owned(),
                updated_at: now,
                connected_since: Some(now),
                hub_protocol: None,
                agent_protocol: None,
                last_notice_at: None,
                detail: None,
            },
        );
        let mut beat = heartbeat(&welcome.config);
        if let Some(overrides) = &self.parts.overrides {
            let refused = overrides.apply_settings(&welcome.config);
            report_refused_overrides(&mut *transport, &refused).await?;
        }
        let mut sent = None;
        // The hub pings; the agent answers `pong`. A silent link for three intervals is dead.
        let mut last_activity = clock.now();
        let mut next_check = clock.now().saturating_add(beat).unwrap_or(clock.now());
        // Report the session's initial status, then only real changes. A status that cannot
        // be read is not sent: the hub must not receive fabricated values.
        let mut last_status = self.parts.status.current();
        if let Some(status) = &last_status {
            frame::send(
                transport.as_mut(),
                &Message::AgentStatus(status.clone()),
                None,
            )
            .await?;
        }
        let mut next_status = clock
            .now()
            .saturating_add(STATUS_PROBE_INTERVAL)
            .unwrap_or(clock.now());
        self.flush(transport.as_mut(), &mut sent).await?;
        let resync = self.parts.resync.clone();
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = async {
                    match &resync {
                        Some(notify) => notify.notified().await,
                        None => std::future::pending().await,
                    }
                } => {
                    // The session stays open, so requests in flight keep their answers.
                    let now = self.skills_now();
                    let message = Message::SkillsUpdate(SkillsUpdate {
                        skills: now.skills,
                        package_skills: now.package_states,
                        catalog_hash: now.extensions.catalog_hash,
                    });
                    frame::send(transport.as_mut(), &message, None).await?;
                    tracing::info!("the package skills changed; told the hub on the open link");
                }
                frame = frame::recv(transport.as_mut()) => {
                    let Some(decoded) = frame? else {
                        return Err(LinkError::Lost("the hub closed the connection".to_owned()));
                    };
                    last_activity = clock.now();
                    self.handle(transport.as_mut(), decoded, &mut beat).await?;
                }
                Some(out) = self.outbox.recv() => {
                    frame::send(transport.as_mut(), &out.message, out.reply_to).await?;
                }
                () = self.parts.spool.pushed() => self.flush(transport.as_mut(), &mut sent).await?,
                () = clock.sleep_until(next_status) => {
                    if let Some(current) = self.parts.status.current()
                        && last_status.as_ref() != Some(&current)
                    {
                        frame::send(
                            transport.as_mut(),
                            &Message::AgentStatus(current.clone()),
                            None,
                        )
                        .await?;
                        last_status = Some(current);
                    }
                    next_status = next_status.saturating_add(STATUS_PROBE_INTERVAL).unwrap_or(next_status);
                }
                () = clock.sleep_until(next_check) => {
                    let silence = clock.now().duration_since(last_activity);
                    let limit = beat.saturating_mul(i32::try_from(MISSED_PONGS).unwrap_or(3));
                    if silence >= limit {
                        return Err(LinkError::Lost(
                            "the hub went silent for three heartbeat intervals".to_owned(),
                        ));
                    }
                    next_check = next_check.saturating_add(beat).unwrap_or(next_check);
                }
            }
        }
    }

    /// Sends every spooled batch the hub has not been sent on this connection.
    async fn flush(
        &self,
        transport: &mut dyn Transport,
        sent: &mut Option<u64>,
    ) -> Result<(), LinkError> {
        let pending = self.parts.spool.lock().await.pending_after(*sent);
        for batch in pending {
            let seq = batch.batch_seq;
            let message = Message::Signals(Signals {
                batch_seq: seq,
                signals: batch.signals,
            });
            frame::send(transport, &message, None).await?;
            *sent = Some(seq);
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        transport: &mut dyn Transport,
        decoded: Decoded,
        beat: &mut SignedDuration,
    ) -> Result<(), LinkError> {
        let id = decoded.id;
        let kind = match decoded.message {
            Message::Pong(_) => return Ok(()),
            Message::Ping(_) => {
                let pong = Message::Pong(Pong {
                    at: self.parts.clock.now(),
                });
                frame::send(transport, &pong, Some(id)).await?;
                return Ok(());
            }
            Message::SignalsAck(ack) => {
                if let Err(err) = self.parts.spool.lock().await.ack(ack.up_to_seq) {
                    tracing::warn!(%err, "could not truncate the spool");
                }
                return Ok(());
            }
            Message::ConfigUpdate(update) => {
                *beat = heartbeat(&update.config);
                if let Some(overrides) = &self.parts.overrides {
                    let refused = overrides.apply_settings(&update.config);
                    report_refused_overrides(transport, &refused).await?;
                }
                return Ok(());
            }
            Message::RunTool(run) => RequestKind::Tool(run),
            Message::RunSkill(run) => RequestKind::Skill(run),
            Message::RunCheck(run) => RequestKind::Check(run),
            Message::GetLogs(get) => RequestKind::Logs(get),
            Message::RunCapability(run) => RequestKind::Capability(run),
            Message::TransferBegin(m) => {
                return self.transfer(transport, id, TransferFrame::Begin(m)).await;
            }
            Message::TransferChunk(m) => {
                return self.transfer(transport, id, TransferFrame::Chunk(m)).await;
            }
            Message::TransferEnd(m) => {
                return self.transfer(transport, id, TransferFrame::End(m)).await;
            }
            Message::ActionStatus(query) => RequestKind::ActionStatus(query),
            Message::GetCatalog(get) => RequestKind::Catalog(get),
            Message::AttestSafety(challenge) => RequestKind::Attest(challenge),
            Message::Unknown { kind } => {
                let reply = Message::Unsupported(Unsupported { type_name: kind });
                frame::send(transport, &reply, Some(id)).await?;
                return Ok(());
            }
            // An older hub of this protocol version cannot take the update, so only a new
            // `hello` can tell it.
            Message::Unsupported(unsupported) if unsupported.type_name == "skills_update" => {
                tracing::warn!("this hub does not know skills_update; upgrade the hub");
                return Err(LinkError::Resync);
            }
            Message::Error(err) => {
                tracing::warn!(
                    code = err.code,
                    message = err.message,
                    "the hub reported an error"
                );
                return Ok(());
            }
            other => {
                tracing::debug!(kind = other.kind(), "ignoring a message");
                return Ok(());
            }
        };
        let request = IncomingRequest { id, kind };
        if let Err(mpsc::error::TrySendError::Full(request)) = self.requests.try_send(request) {
            let reply = crate::requests::busy_reply(&request.kind);
            frame::send(transport, &reply, Some(id)).await?;
        }
        Ok(())
    }
}

/// Writes `status`, warning rather than failing when the file cannot be written.
fn write_status(dir: Option<&Path>, status: &LinkStatus) {
    let Some(dir) = dir else {
        return;
    };
    if let Err(err) = self::status::write(dir, status) {
        tracing::warn!(%err, "could not write the link status file");
    }
}

/// Tells the hub which check overrides this agent refused, so the owner sees why a setting
/// did not take effect. Sends nothing when none were refused.
async fn report_refused_overrides(
    transport: &mut dyn Transport,
    refused: &[String],
) -> Result<(), LinkError> {
    if refused.is_empty() {
        return Ok(());
    }
    tracing::warn!(refused = ?refused, "refused check overrides from the hub");
    let message = Message::Error(domovoi_proto::messages::ErrorMessage {
        code: "check_override_refused".to_owned(),
        message: refused.join("; "),
    });
    frame::send(transport, &message, None).await?;
    Ok(())
}
