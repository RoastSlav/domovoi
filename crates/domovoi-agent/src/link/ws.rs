//! The WebSocket transport over pinned TLS, and dialling the hub.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::Clock;
use domovoi_proto::{MAX_FRAME_BYTES, Transport, TransportError};
use futures::{SinkExt, StreamExt};
use jiff::SignedDuration;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};
use url::Url;

use super::tls::{channel_binding, client_config, is_rejected_certificate};
use crate::error::LinkError;
use crate::time::within;

/// How long dialling, TLS and the WebSocket upgrade may take together.
pub const CONNECT_TIMEOUT: SignedDuration = SignedDuration::from_secs(10);

/// A live connection to the hub.
pub struct Connected {
    /// The framed transport.
    pub transport: Box<dyn Transport>,
    /// What binds the challenge signature to this connection (the TLS channel binding).
    pub session_hint: String,
}

/// Opens connections to the hub. The runtime is generic over it so single-box mode can
/// supply an in-memory pair.
#[async_trait]
pub trait Connector: Send + Sync {
    /// Opens one connection.
    async fn connect(&self) -> Result<Connected, LinkError>;
}

/// The protocol's text frames over a WebSocket.
pub struct WsTransport<S> {
    ws: WebSocketStream<S>,
}

impl<S> WsTransport<S> {
    /// Wraps an upgraded WebSocket.
    pub fn new(ws: WebSocketStream<S>) -> Self {
        Self { ws }
    }
}

fn io(err: WsError) -> TransportError {
    match err {
        WsError::ConnectionClosed | WsError::AlreadyClosed => TransportError::Closed,
        other => TransportError::Io(other.to_string()),
    }
}

#[async_trait]
impl<S> Transport for WsTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        self.ws.send(WsMessage::text(frame)).await.map_err(io)
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        loop {
            match self.ws.next().await {
                None | Some(Ok(WsMessage::Close(_))) => return Ok(None),
                Some(Err(WsError::ConnectionClosed | WsError::AlreadyClosed)) => return Ok(None),
                Some(Err(err)) => return Err(io(err)),
                Some(Ok(WsMessage::Text(text))) => return Ok(Some(text.as_str().to_owned())),
                Some(Ok(WsMessage::Binary(_))) => {
                    return Err(TransportError::Io("binary frames are not used".to_owned()));
                }
                // Pings are answered by the WebSocket layer itself.
                Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_))) => {}
            }
        }
    }
}

/// The WebSocket limits: one protocol frame at most.
#[must_use]
pub fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

/// Dials `url` (`wss://`), trusting only what `verifier` accepts.
pub async fn dial(
    url: &Url,
    verifier: Arc<dyn ServerCertVerifier>,
    clock: &dyn Clock,
) -> Result<Connected, LinkError> {
    let bad = |why: &str| LinkError::BadUrl(url.to_string(), why.to_owned());
    if url.scheme() != "wss" {
        return Err(bad("the scheme must be wss"));
    }
    let host = url.host_str().ok_or_else(|| bad("it has no host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let name = ServerName::try_from(host.trim_matches(['[', ']']).to_owned())
        .map_err(|e| bad(&e.to_string()))?;
    let config = client_config(verifier).map_err(|e| LinkError::Connect(e.to_string()))?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let addr = format!("{}:{port}", host);
    let attempt = async {
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|e| LinkError::Connect(e.to_string()))?;
        let tls: TlsStream<TcpStream> = connector.connect(name, tcp).await.map_err(|e| {
            if is_rejected_certificate(&e) {
                LinkError::CertificateMismatch
            } else {
                LinkError::Connect(e.to_string())
            }
        })?;
        let session_hint =
            channel_binding(tls.get_ref().1).map_err(|e| LinkError::Connect(e.to_string()))?;
        let (ws, _) =
            tokio_tungstenite::client_async_with_config(url.as_str(), tls, Some(ws_config()))
                .await
                .map_err(|e| LinkError::Connect(e.to_string()))?;
        Ok(Connected {
            transport: Box::new(WsTransport::new(ws)),
            session_hint,
        })
    };
    within(clock, CONNECT_TIMEOUT, attempt)
        .await
        .unwrap_or(Err(LinkError::Timeout))
}

/// Dials the hub with the pinned certificate.
pub struct TlsConnector {
    url: Url,
    verifier: Arc<dyn ServerCertVerifier>,
    clock: Arc<dyn Clock>,
}

impl TlsConnector {
    /// A connector for `url` that trusts only `verifier`.
    #[must_use]
    pub fn new(url: Url, verifier: Arc<dyn ServerCertVerifier>, clock: Arc<dyn Clock>) -> Self {
        Self {
            url,
            verifier,
            clock,
        }
    }
}

#[async_trait]
impl Connector for TlsConnector {
    async fn connect(&self) -> Result<Connected, LinkError> {
        dial(&self.url, self.verifier.clone(), &*self.clock).await
    }
}
