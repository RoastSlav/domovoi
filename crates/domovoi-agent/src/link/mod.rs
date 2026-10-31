//! The link to the hub: connect, authenticate, exchange messages, reconnect.
//!
//! - [`tls`] pins the hub certificate; [`ws`] frames the protocol over a WebSocket.
//! - [`handshake`] proves the agent holds its key, bound to the TLS session.
//! - [`Link::run`] reconnects with [`backoff`] until shutdown, revocation or an
//!   incompatible hub. Each connection replays the spool, sends a `ping` every heartbeat
//!   and gives up after three unanswered pings.
//! - Requests (`run_tool`, `run_skill`, `run_check`) are forwarded to the request handler
//!   and never handled on the link task.

pub mod backoff;
pub mod frame;
pub mod handshake;
pub mod tls;
pub mod ws;

use std::sync::Arc;

use domovoi_core::ports::{Clock, HostFs};
use domovoi_core::{RequestId, SkillSummary};
use domovoi_proto::messages::{Ping, Pong, RunCheck, RunSkill, RunTool, Signals, Unsupported};
use domovoi_proto::{Decoded, Message, Transport};
use jiff::SignedDuration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use self::backoff::{Backoff, STABLE_AFTER};
use self::handshake::{Announce, STEP_TIMEOUT, handshake, heartbeat};
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

/// A request from the hub, waiting for the request handler.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestKind {
    /// Run one read tool.
    Tool(RunTool),
    /// Run one approved skill.
    Skill(RunSkill),
    /// Run one check now.
    Check(RunCheck),
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
    /// Signal batches to send and replay.
    pub spool: Arc<SharedSpool>,
    /// Jitter factors for the reconnect delay.
    pub jitter: Box<dyn FnMut() -> f64 + Send + Sync>,
}

/// The hub link task.
pub struct Link {
    parts: LinkParts,
    backoff: Backoff,
    outbox: mpsc::Receiver<Outgoing>,
    requests: mpsc::Sender<IncomingRequest>,
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
            outbox,
            requests,
        };
        (link, handle, requests_rx)
    }

    /// Connects and serves the hub until `shutdown`, or until the hub revokes this agent or
    /// speaks an incompatible protocol.
    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), LinkError> {
        let clock = self.parts.clock.clone();
        loop {
            let attempt = tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                attempt = self.parts.connector.connect() => attempt,
            };
            match attempt {
                Ok(connected) => {
                    let started = clock.now();
                    let result = self.session(connected, &shutdown).await;
                    if clock.now().duration_since(started) >= STABLE_AFTER {
                        self.backoff.reset();
                    }
                    match result {
                        Ok(()) => return Ok(()),
                        Err(err) if err.is_final() => {
                            tracing::error!(%err, "the hub link stopped");
                            return Err(err);
                        }
                        Err(err) => tracing::warn!(%err, "the hub link dropped"),
                    }
                }
                Err(err) if err.is_final() => return Err(err),
                Err(err) => tracing::warn!(%err, "could not reach the hub"),
            }
            let delay = self.backoff.next_delay();
            tracing::debug!(delay_ms = delay.as_millis(), "reconnecting");
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = sleep(&*clock, delay) => {}
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
        let announce = Announce {
            facts: host_facts(&*self.parts.fs).await,
            skills: &self.parts.skills,
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
        tracing::info!(session = ?welcome.session_id, "connected to the hub");
        let mut beat = heartbeat(&welcome.config);
        let mut sent = None;
        let mut missed = 0;
        let mut next_ping = clock.now().saturating_add(beat).unwrap_or(clock.now());
        self.flush(transport.as_mut(), &mut sent).await?;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                frame = frame::recv(transport.as_mut()) => {
                    let Some(decoded) = frame? else {
                        return Err(LinkError::Lost("the hub closed the connection".to_owned()));
                    };
                    self.handle(transport.as_mut(), decoded, &mut missed, &mut beat).await?;
                }
                Some(out) = self.outbox.recv() => {
                    frame::send(transport.as_mut(), &out.message, out.reply_to).await?;
                }
                () = self.parts.spool.pushed() => self.flush(transport.as_mut(), &mut sent).await?,
                () = clock.sleep_until(next_ping) => {
                    if missed >= MISSED_PONGS {
                        return Err(LinkError::Lost(format!("{missed} pings went unanswered")));
                    }
                    let ping = Message::Ping(Ping { at: clock.now() });
                    frame::send(transport.as_mut(), &ping, None).await?;
                    missed += 1;
                    next_ping = next_ping.saturating_add(beat).unwrap_or(next_ping);
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
        missed: &mut u32,
        beat: &mut SignedDuration,
    ) -> Result<(), LinkError> {
        let id = decoded.id;
        let kind = match decoded.message {
            Message::Pong(_) => {
                *missed = 0;
                return Ok(());
            }
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
                return Ok(());
            }
            Message::RunTool(run) => RequestKind::Tool(run),
            Message::RunSkill(run) => RequestKind::Skill(run),
            Message::RunCheck(run) => RequestKind::Check(run),
            Message::Unknown { kind } => {
                let reply = Message::Unsupported(Unsupported { kind });
                frame::send(transport, &reply, Some(id)).await?;
                return Ok(());
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
