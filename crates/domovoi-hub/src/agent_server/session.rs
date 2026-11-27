//! The message loop of an authenticated agent session.

use std::collections::HashMap;
use std::net::IpAddr;

use domovoi_core::HostId;
use domovoi_proto::messages::{
    AUTHORITATIVE_ABSENCE, Ping, Pong, SessionId, SignalsAck, Unsupported,
};
use domovoi_proto::{Decoded, Message, Transport};
use jiff::SignedDuration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
        identity: domovoi_core::AgentSessionId::new(),
        agent: agent.agent,
        since: inner.deps.clock.now(),
        version: hello.agent_version,
        build: hello.agent_build,
        facts: *hello.facts,
        skills: hello.skills,
        package_skills: hello.package_skills,
        catalog_hash: hello.catalog_hash.map(|hash| hex::encode(hash.as_bytes())),
        runs_as_root: hello.runs_as_root,
        in_container: hello.in_container,
        skill_runner_status: hello.skill_runner_status,
        helper_status: hello.helper_status,
        helper_identity: hello
            .helper_identity
            .map(|identity| *identity.public_key.as_bytes()),
        ceiling_summary: hello.ceiling_summary,
        ceiling_hash: hello.ceiling_hash.map(|hash| hex::encode(hash.as_bytes())),
        checks: hello.checks,
        docker: hello.docker,
        journal_epoch: hello.journal_epoch,
        authoritative_absence: hello
            .features
            .iter()
            .any(|feature| feature == AUTHORITATIVE_ABSENCE),
        agent_status: None,
        override_refusals: Vec::new(),
        outbox,
        pending: HashMap::new(),
        cancel: cancel.clone(),
    };
    let journal_epoch = live.journal_epoch;
    let authoritative_absence = live.authoritative_absence;
    // Test-only: let a test retire the agent between the handshake and this registration.
    #[cfg(test)]
    if let Some(gate) = inner.registry.registration_gate() {
        // The permit is only a release signal; registration itself is the next step.
        drop(gate.acquire_owned().await.map_err(|_| ConnError::Closed)?);
    }
    // The handshake proved the key, but enrollment may have changed while the agent answered.
    // The accepted identity must still be the host's current one before it is registered.
    let current = inner.deps.hosts.live_agent(host).await?;
    let same_identity = current.as_ref().is_some_and(|current| {
        current.agent == agent.agent
            && current.host == agent.host
            && current.public_key == agent.public_key
    });
    if !same_identity {
        tracing::info!(
            %host,
            agent = %agent.agent,
            "refused a session for an identity that is no longer current"
        );
        let _ = transport.close().await;
        return Ok(());
    }
    if let Err(err) = inner.registry.register(host, live, remote_ip) {
        tracing::info!(%host, agent = %agent.agent, %err, "refused a session for a retired agent");
        let _ = transport.close().await;
        return Ok(());
    }
    tracing::info!(
        %host,
        agent = %agent.agent,
        %journal_epoch,
        authoritative_absence,
        "agent connected"
    );
    if let (Some(store), Some(skills)) = (
        inner.package_reports.get(),
        inner.registry.package_skills(host),
    ) {
        super::package_report::save(&**store, host, &skills).await;
    }
    reconcile(inner, host, &cancel);
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

/// Tells the action service that this host's agent authenticated, so its dispatched runs
/// are reconciled against the journal.
///
/// The task belongs to the session that triggered it: when that session ends or is
/// replaced, the task stops asking. An answer the pass already received is still committed:
/// the pass hands each outcome to the writer before it awaits anything that a cancelled task
/// could drop.
pub(super) fn reconcile(inner: &Inner, host: HostId, session: &CancellationToken) {
    let Some(reconciler) = inner.reconciler.get().cloned() else {
        return;
    };
    let shutdown = inner.shutdown.clone();
    let session = session.clone();
    tokio::spawn(async move {
        tokio::select! {
            () = shutdown.cancelled() => {}
            () = session.cancelled() => {}
            () = reconciler.reconcile_host(host) => {}
        }
    });
}

/// Tells the status listener, if the runtime set one, that `host` reported new runtime
/// status. The listener runs in its own task, bounded by the server's shutdown.
fn notify_status(inner: &Inner, host: HostId) {
    let Some(listener) = inner.status_listener.get().cloned() else {
        return;
    };
    let shutdown = inner.shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            () = shutdown.cancelled() => {}
            () = listener.reconcile_host(host) => {}
        }
    });
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
            // Keys the agent's checks stopped reporting are kept as history and shown as
            // "no longer reported".
            let at = inner.deps.clock.now();
            for key in &digest.retired {
                if let Err(err) = inner.deps.facts.retire(host, key, at).await {
                    tracing::warn!(%host, %err, key, "could not retire a fact");
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
        // The session stays: requests in flight keep their answers, and nothing was lost
        // that a reconciliation would have to settle.
        Message::SkillsUpdate(update) => {
            let changed = inner.registry.set_skills(host, session, update);
            if changed
                && let (Some(store), Some(skills)) = (
                    inner.package_reports.get(),
                    inner.registry.package_skills(host),
                )
            {
                super::package_report::save(&**store, host, &skills).await;
            }
            tracing::info!(%host, "the agent announced a new skill list");
            Ok(())
        }
        Message::AgentStatus(status) => {
            inner.registry.set_agent_status(host, session, status);
            notify_status(inner, host);
            Ok(())
        }
        Message::Error(err) if err.code == "check_override_refused" && reply_to.is_none() => {
            tracing::warn!(%host, refused = %err.message, "the agent refused check overrides");
            inner.registry.set_override_refusals(
                host,
                session,
                err.message.split("; ").map(str::to_owned).collect(),
            );
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
