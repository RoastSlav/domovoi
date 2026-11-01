//! The message loop of an authenticated agent session.

use std::collections::HashMap;
use std::net::IpAddr;

use domovoi_core::HostId;
use domovoi_proto::messages::{Pong, SessionId, SignalsAck, Unsupported};
use domovoi_proto::{Decoded, Message, Transport};
use tokio::sync::mpsc;

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
        skills: hello.skills,
        outbox,
        pending: HashMap::new(),
        cancel: cancel.clone(),
    };
    inner.registry.register(host, live, remote_ip);
    tracing::info!(%host, agent = %agent.agent, "agent connected");
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
                    handle(inner, transport, host, session, decoded).await?;
                }
            }
        }
    }
    .await;
    inner.registry.end(host, session);
    result
}

async fn handle(
    inner: &Inner,
    transport: &mut dyn Transport,
    host: HostId,
    session: SessionId,
    decoded: Decoded,
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
        Message::Pong(_) => Ok(()),
        reply @ (Message::ToolResult(_) | Message::SkillResult(_) | Message::CheckResult(_)) => {
            let delivered =
                reply_to.is_some_and(|to| inner.registry.deliver(host, session, to, reply));
            if !delivered {
                tracing::debug!(%host, "a result arrived after its request gave up");
            }
            Ok(())
        }
        Message::Error(err) => {
            tracing::warn!(%host, code = err.code, message = err.message, "agent reported an error");
            Ok(())
        }
        Message::Unknown { kind } => {
            send(
                transport,
                &Message::Unsupported(Unsupported { kind }),
                Some(id),
            )
            .await
        }
        other => {
            let kind = other.kind().to_owned();
            send(
                transport,
                &Message::Unsupported(Unsupported { kind }),
                Some(id),
            )
            .await
        }
    }
}
