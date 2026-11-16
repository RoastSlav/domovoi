//! The message loop of an authenticated agent session.

use std::collections::HashMap;
use std::net::IpAddr;

use domovoi_core::HostId;
use domovoi_proto::messages::{Ping, Pong, SessionId, SignalsAck, Unsupported};
use domovoi_proto::{Decoded, Message, Transport};
use jiff::SignedDuration;
use tokio::sync::mpsc;

/// Hub pings that may go unanswered before the session counts as dead.
const MISSED_PONGS: u32 = 3;

use super::Inner;
use super::conn::{ConnError, recv, send};
use super::handshake::Accepted;
use super::registry::{Live, MAX_PENDING};

/// Registers the session, serves it until it ends, and unregisters it.
pub(crate) async fn run_session(
    inner: &Inner,
    transport: &mut dyn Transport,
    accepted: Accepted,
    remote_ip: Option<IpAddr>,
) -> Result<(), ConnError> {
    let Accepted {
        agent,
        hello,
        session,
    } = accepted;
    let host = agent.host;
    let (outbox, mut queued) = mpsc::channel(MAX_PENDING);
    let cancel = inner.shutdown.child_token();
    let live = Live {
        session,
        since: inner.deps.clock.now(),
        version: hello.agent_version,
        facts: *hello.facts,
        skills: hello.skills,
        checks: hello.checks,
        docker: hello.docker,
        journal_epoch: hello.journal_epoch,
        agent_status: None,
        outbox,
        pending: HashMap::new(),
        cancel: cancel.clone(),
    };
    let journal_epoch = live.journal_epoch;
    inner.registry.register(host, live, remote_ip);
    tracing::info!(%host, agent = %agent.agent, %journal_epoch, "agent connected");
    let clock = inner.deps.clock.clone();
    let secs = inner.settings.heartbeat.as_secs().max(1);
    let beat = SignedDuration::from_secs(secs);
    let mut missed = 0u32;
    let mut next_ping = clock.now().saturating_add(beat).unwrap_or(clock.now());
    let result = async {
        loop {
            tokio::select! {
                () = cancel.cancelled() => {
                    transport.close().await.map_err(|e| ConnError::Protocol(e.to_string()))?;
                    return Ok(());
                }
                Some(frame) = queued.recv() => transport
                    .send(frame)
                    .await
                    .map_err(|e| ConnError::Protocol(e.to_string()))?,
                frame = recv(transport) => {
                    let Some(decoded) = frame? else {
                        return Err(ConnError::Closed);
                    };
                    inner.registry.seen(host, inner.deps.clock.now());
                    handle(inner, transport, host, session, decoded, &mut missed).await?;
                }
                () = clock.sleep_until(next_ping) => {
                    if missed >= MISSED_PONGS {
                        return Err(ConnError::Closed);
                    }
                    let ping = Message::Ping(Ping { at: clock.now() });
                    send(transport, &ping, None).await?;
                    missed += 1;
                    next_ping = next_ping.saturating_add(beat).unwrap_or(next_ping);
                }
            }
        }
    }
    .await;
    inner.registry.end(host, session, inner.deps.clock.now());
    result
}

async fn handle(
    inner: &Inner,
    transport: &mut dyn Transport,
    host: HostId,
    session: SessionId,
    decoded: Decoded,
    missed: &mut u32,
) -> Result<(), ConnError> {
    let Decoded {
        id,
        reply_to,
        message,
    } = decoded;
    match message {
        Message::Signals(batch) => {
            let (ours, theirs): (Vec<_>, Vec<_>) =
                batch.signals.into_iter().partition(|s| s.host() == host);
            if !theirs.is_empty() {
                tracing::warn!(%host, dropped = theirs.len(), "signals for another host dropped");
            }
            if !ours.is_empty()
                && let Err(err) = inner.deps.sink.submit(ours).await
            {
                // Not acknowledged, so the agent keeps the batch and sends it again.
                tracing::warn!(%host, %err, "could not take a signal batch");
                return Ok(());
            }
            let ack = Message::SignalsAck(SignalsAck {
                up_to_seq: batch.batch_seq,
            });
            send(transport, &ack, Some(id)).await
        }
        Message::DigestFacts(digest) => {
            for fact in digest.facts.iter().filter(|f| f.host == host) {
                if let Err(err) = inner.deps.facts.record(fact).await {
                    tracing::warn!(%host, %err, key = fact.key, "could not record a fact");
                }
            }
            Ok(())
        }
        Message::Ping(_) => {
            let pong = Message::Pong(Pong {
                at: inner.deps.clock.now(),
            });
            send(transport, &pong, Some(id)).await
        }
        Message::Pong(_) => {
            *missed = 0;
            Ok(())
        }
        Message::AgentStatus(status) => {
            inner.registry.set_agent_status(host, session, status);
            Ok(())
        }
        reply @ (Message::ToolResult(_)
        | Message::SkillResult(_)
        | Message::CheckResult(_)
        | Message::Logs(_)
        | Message::CapabilityResult(_)
        | Message::Refused(_)
        | Message::ActionStatusAnswers(_)
        | Message::CatalogPage(_)
        | Message::TransferResult(_)
        | Message::SafetyAttestation(_)
        | Message::Unsupported(_)
        | Message::Error(_)) => {
            let reported: Option<(String, String)> = match &reply {
                Message::Error(err) => Some((err.code.to_owned(), err.message.to_owned())),
                Message::Refused(refused) => {
                    Some((refused.code.raw().to_owned(), refused.detail.to_owned()))
                }
                _ => None,
            };
            let delivered =
                reply_to.is_some_and(|to| inner.registry.deliver(host, session, to, reply));
            if !delivered {
                // A busy or failed request the hub was not waiting for stays visible in
                // the log; a request the hub is waiting for gets the error delivered.
                match reported {
                    Some((code, message)) => tracing::warn!(
                        %host,
                        code = code,
                        message = message,
                        "agent reported an error nobody was waiting for"
                    ),
                    _ => tracing::debug!(
                        %host,
                        "a result arrived after its request gave up"
                    ),
                }
            }
            Ok(())
        }
        Message::Unknown { kind } => {
            send(
                transport,
                &Message::Unsupported(Unsupported { type_name: kind }),
                Some(id),
            )
            .await
        }
        other => {
            let type_name = other.kind().to_owned();
            send(
                transport,
                &Message::Unsupported(Unsupported { type_name }),
                Some(id),
            )
            .await
        }
    }
}
