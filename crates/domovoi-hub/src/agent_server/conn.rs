//! One agent connection, from the TLS accept to the end of its session.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};

use domovoi_core::RequestId;
use domovoi_core::ports::{Clock, StoreError};
use domovoi_proto::messages::RejectReason;
use domovoi_proto::{Decoded, Message, Transport, decode, encode};
use jiff::SignedDuration;
use tokio::net::TcpStream;

use super::Inner;
use super::handshake::{FIRST_FRAME_TIMEOUT, Shaken, handshake};
use super::session::run_session;
use super::tls::channel_binding;
use super::ws::{Limited, WsTransport, ws_config};

/// Why a connection ended early.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnError {
    /// The peer closed the connection.
    #[error("the connection closed")]
    Closed,
    /// The peer took too long.
    #[error("the agent took too long")]
    Timeout,
    /// The peer broke the protocol or the transport failed.
    #[error("{0}")]
    Protocol(String),
    /// The agent was refused.
    #[error("rejected: {0:?}")]
    Rejected(RejectReason),
    /// The store failed.
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// Runs `fut` until it finishes or `limit` passes on `clock`; `None` on timeout.
pub(crate) async fn within<F: Future>(
    clock: &dyn Clock,
    limit: SignedDuration,
    fut: F,
) -> Option<F::Output> {
    let now = clock.now();
    let deadline = now.saturating_add(limit).unwrap_or(now);
    tokio::select! {
        out = fut => Some(out),
        () = clock.sleep_until(deadline) => None,
    }
}

/// Encodes and sends `message`, as a reply to `reply_to` when given.
pub(crate) async fn send(
    transport: &mut dyn Transport,
    message: &Message,
    reply_to: Option<RequestId>,
) -> Result<(), ConnError> {
    let frame = encode(message, RequestId::new(), reply_to)
        .map_err(|e| ConnError::Protocol(e.to_string()))?;
    transport
        .send(frame)
        .await
        .map_err(|e| ConnError::Protocol(e.to_string()))
}

/// Receives and decodes the next message; `None` when the peer closed the connection.
pub(crate) async fn recv(transport: &mut dyn Transport) -> Result<Option<Decoded>, ConnError> {
    let Some(frame) = transport
        .recv()
        .await
        .map_err(|e| ConnError::Protocol(e.to_string()))?
    else {
        return Ok(None);
    };
    decode(&frame)
        .map(Some)
        .map_err(|e| ConnError::Protocol(e.to_string()))
}

/// Receives the next message, treating a closed connection as an error.
pub(crate) async fn expect(transport: &mut dyn Transport) -> Result<Decoded, ConnError> {
    recv(transport).await?.ok_or(ConnError::Closed)
}

/// Accepts TLS and the WebSocket upgrade on `tcp`, then runs the connection.
pub(crate) async fn accept(inner: &Inner, tcp: TcpStream, peer: SocketAddr) {
    let upgrade = async {
        let tls = inner
            .acceptor
            .accept(tcp)
            .await
            .map_err(|e| ConnError::Protocol(format!("TLS: {e}")))?;
        let binding = channel_binding(tls.get_ref().1)
            .map_err(|e| ConnError::Protocol(format!("channel binding: {e}")))?;
        let ws = tokio_tungstenite::accept_async_with_config(tls, Some(ws_config()))
            .await
            .map_err(|e| ConnError::Protocol(format!("WebSocket: {e}")))?;
        Ok::<_, ConnError>((WsTransport::new(ws), binding))
    };
    match within(&*inner.deps.clock, FIRST_FRAME_TIMEOUT, upgrade).await {
        Some(Ok((transport, binding))) => {
            run(inner, Box::new(transport), &binding, Some(peer.ip())).await;
        }
        Some(Err(err)) => tracing::debug!(%peer, %err, "agent connection refused"),
        None => tracing::debug!(%peer, "agent connection timed out before the upgrade"),
    }
}

/// Runs the handshake and, for an authenticated agent, its session.
pub(crate) async fn run(
    inner: &Inner,
    transport: Box<dyn Transport>,
    binding: &str,
    remote_ip: Option<IpAddr>,
) {
    let mut transport = Limited::new(transport, inner.deps.clock.clone());
    match handshake(inner, &mut transport, binding).await {
        Ok(Shaken::Enrolled) => {}
        Ok(Shaken::Session(accepted)) => {
            let host = accepted.agent.host;
            if let Err(err) = run_session(inner, &mut transport, *accepted, remote_ip).await {
                tracing::info!(%host, %err, "agent session ended");
            }
        }
        Err(err) => tracing::info!(%err, "agent handshake failed"),
    }
}
