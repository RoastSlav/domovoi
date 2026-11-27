//! Plain-sentence log lines about agent connections: why a handshake failed and how a
//! session ended, with the peer, the host's name and id, and no library text or links.

use std::net::IpAddr;

use domovoi_core::log_text::{human_duration, without_urls};
use domovoi_core::{AgentId, HostId};
use domovoi_proto::messages::{RejectReason, SessionId};

use super::Inner;
use super::conn::ConnError;

/// What is known about the other end while its handshake runs, so a failure can name it.
#[derive(Debug, Default)]
pub(crate) struct Peer {
    /// The agent id the peer claimed or that an invite belongs to.
    pub agent: Option<AgentId>,
    /// The host the agent id or the invite belongs to.
    pub host: Option<HostId>,
}

/// How a session ended, for the level and the wording of its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndKind {
    /// The agent went away (stopped or closed the connection); nothing to act on.
    Disconnected,
    /// The hub ended the session (shutdown, or the agent was replaced).
    ClosedByHub,
    /// The agent stopped answering heartbeats.
    Timeout,
    /// The TLS layer failed.
    Tls,
    /// The agent sent something the hub could not accept.
    Protocol,
    /// The hub's own store failed.
    Store,
}

/// Why the hub refused an agent, as one sentence a person can act on.
#[must_use]
pub(crate) fn reject_sentence(reason: &RejectReason) -> String {
    match reason {
        RejectReason::UnknownAgent => "the hub does not know this agent id: it was never enrolled here, or the hub data was reset; enroll it again".to_owned(),
        RejectReason::Revoked => "the agent was revoked or replaced by a newer enrollment; enroll it again".to_owned(),
        RejectReason::Incompatible { hub } => format!("the agent protocol version does not match the hub ({hub}); update the older one"),
        RejectReason::AgentTooOld { .. } => "the agent is older than the oldest release this hub accepts; update it".to_owned(),
        RejectReason::BadSignature => "the agent could not prove its identity: the challenge signature did not verify".to_owned(),
        RejectReason::BadToken => "the enrollment token is unknown, expired or already used (tokens are single use); create a new invite".to_owned(),
        RejectReason::Unknown => "the hub refused the agent for a reason this build has no name for".to_owned(),
    }
}

/// Why a connection failed before a session started, as one sentence.
#[must_use]
pub(crate) fn failure_sentence(err: &ConnError) -> String {
    match err {
        ConnError::Rejected(reason) => reject_sentence(reason),
        ConnError::Timeout => "the agent did not send the next frame in time".to_owned(),
        ConnError::Closed => "the agent closed the connection during the handshake".to_owned(),
        ConnError::Protocol(text) => without_urls(text),
        ConnError::Store(err) => format!("the hub store failed: {err}"),
    }
}

/// Sorts how a session ended, with the sentence that says why.
#[must_use]
pub(crate) fn classify(result: &Result<(), ConnError>) -> (EndKind, String) {
    match result {
        Ok(()) => (
            EndKind::ClosedByHub,
            "the hub closed the session (shutdown, or a newer connection replaced it)".to_owned(),
        ),
        Err(ConnError::Closed) => (
            EndKind::Disconnected,
            "the agent closed the connection".to_owned(),
        ),
        Err(ConnError::Timeout) => (
            EndKind::Timeout,
            "the agent missed three heartbeats in a row".to_owned(),
        ),
        Err(ConnError::Store(err)) => (EndKind::Store, format!("the hub store failed: {err}")),
        Err(ConnError::Rejected(reason)) => (EndKind::Protocol, reject_sentence(reason)),
        Err(ConnError::Protocol(text)) => {
            let lower = text.to_lowercase();
            if lower.contains("close_notify")
                || lower.contains("reset by peer")
                || lower.contains("broken pipe")
            {
                (
                    EndKind::Disconnected,
                    "the connection ended without a goodbye: the agent stopped, was killed, or lost its network".to_owned(),
                )
            } else if lower.contains("tls") || lower.contains("certificate") {
                (EndKind::Tls, format!("TLS failed: {}", without_urls(text)))
            } else {
                (EndKind::Protocol, without_urls(text))
            }
        }
    }
}

/// The name of `host`, or `unknown`.
pub(crate) async fn host_name(inner: &Inner, host: HostId) -> String {
    inner
        .deps
        .hosts
        .get_host(host)
        .await
        .ok()
        .flatten()
        .map_or_else(|| "unknown".to_owned(), |h| h.name.to_string())
}

fn ip_text(ip: Option<IpAddr>) -> String {
    ip.map_or_else(|| "local".to_owned(), |ip| ip.to_string())
}

/// Logs a handshake that did not end in a session: who it was, and why in one sentence.
pub(crate) async fn log_handshake_failure(
    inner: &Inner,
    ip: Option<IpAddr>,
    peer: &Peer,
    err: &ConnError,
) {
    let name = match peer.host {
        Some(host) => host_name(inner, host).await,
        None => "-".to_owned(),
    };
    let host_id = peer.host.map_or_else(|| "-".to_owned(), |h| h.to_string());
    let agent_id = peer.agent.map_or_else(|| "-".to_owned(), |a| a.to_string());
    let peer_ip = ip_text(ip);
    let reason = failure_sentence(err);
    if matches!(err, ConnError::Store(_)) {
        tracing::error!(peer = %peer_ip, host = %name, %host_id, %agent_id, %reason, "agent handshake failed");
    } else if matches!(err, ConnError::Closed) {
        tracing::info!(peer = %peer_ip, host = %name, %host_id, %agent_id, %reason, "agent handshake failed");
    } else {
        tracing::warn!(peer = %peer_ip, host = %name, %host_id, %agent_id, %reason, "agent handshake failed");
    }
}

/// Logs the end of a session: how long it lasted and how it ended.
pub(crate) async fn log_session_end(
    inner: &Inner,
    host: HostId,
    session: SessionId,
    ip: Option<IpAddr>,
    uptime_secs: u64,
    result: &Result<(), ConnError>,
) {
    let (kind, reason) = classify(result);
    let name = host_name(inner, host).await;
    let uptime = human_duration(uptime_secs);
    let session = session.to_string();
    let peer = ip_text(ip);
    match kind {
        EndKind::Disconnected => {
            tracing::info!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent disconnected after {uptime}");
        }
        EndKind::ClosedByHub => {
            tracing::info!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent session closed by the hub after {uptime}");
        }
        EndKind::Timeout => {
            tracing::warn!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent stopped answering after {uptime}");
        }
        EndKind::Tls => {
            tracing::warn!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent connection failed after {uptime}");
        }
        EndKind::Protocol => {
            tracing::warn!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent session ended on a protocol error after {uptime}");
        }
        EndKind::Store => {
            tracing::error!(host = %name, host_id = %host, %session, %peer, %uptime, %reason, "agent session ended on a store error after {uptime}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_without_a_goodbye_is_a_disconnect_not_a_fault() {
        let err = ConnError::Protocol(
            "transport error: IO error: peer closed connection without sending TLS close_notify: https://docs.rs/rustls/0.23/x"
                .to_owned(),
        );
        let (kind, reason) = classify(&Err(err));
        assert_eq!(kind, EndKind::Disconnected);
        assert!(!reason.contains("http"), "{reason}");
    }

    #[test]
    fn other_endings_keep_their_class() {
        assert_eq!(classify(&Err(ConnError::Timeout)).0, EndKind::Timeout);
        assert_eq!(classify(&Err(ConnError::Closed)).0, EndKind::Disconnected);
        assert_eq!(classify(&Ok(())).0, EndKind::ClosedByHub);
        let tls = ConnError::Protocol("TLS: bad certificate: https://x.example/y".to_owned());
        let (kind, reason) = classify(&Err(tls));
        assert_eq!(kind, EndKind::Tls);
        assert!(!reason.contains("http"), "{reason}");
        let other =
            ConnError::Protocol("the first frame must be enroll or hello, not ping".to_owned());
        assert_eq!(classify(&Err(other)).0, EndKind::Protocol);
    }

    #[test]
    fn every_refusal_reads_as_a_sentence() {
        for reason in [
            RejectReason::UnknownAgent,
            RejectReason::Revoked,
            RejectReason::BadSignature,
            RejectReason::BadToken,
            RejectReason::Unknown,
        ] {
            let text = reject_sentence(&reason);
            assert!(text.contains(' ') && !text.contains("BadToken"), "{text}");
        }
    }
}
