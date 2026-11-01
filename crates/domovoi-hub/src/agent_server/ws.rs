//! The WebSocket transport, the frame limits and the per-connection frame rate.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::Clock;
use domovoi_proto::{MAX_FRAME_BYTES, Transport, TransportError};
use futures::{SinkExt, StreamExt};
use jiff::{SignedDuration, Timestamp};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};

/// The most frames one connection may send in a second. More closes the connection.
pub const MAX_FRAMES_PER_SECOND: u32 = 100;

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

    async fn close(&mut self) -> Result<(), TransportError> {
        let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Away,
            reason: "going_away".into(),
        };
        self.ws
            .send(WsMessage::Close(Some(frame)))
            .await
            .map_err(io)
    }
}

/// The WebSocket limits: a message or frame larger than one protocol frame is refused.
#[must_use]
pub fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

/// Wraps a transport and fails it when the peer sends too many frames or one too large.
pub struct Limited {
    inner: Box<dyn Transport>,
    clock: Arc<dyn Clock>,
    window: Timestamp,
    frames: u32,
}

impl Limited {
    /// Applies the limits to `inner`, counting time on `clock`.
    #[must_use]
    pub fn new(inner: Box<dyn Transport>, clock: Arc<dyn Clock>) -> Self {
        let window = clock.now();
        Self {
            inner,
            clock,
            window,
            frames: 0,
        }
    }
}

#[async_trait]
impl Transport for Limited {
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        self.inner.send(frame).await
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        let Some(frame) = self.inner.recv().await? else {
            return Ok(None);
        };
        if frame.len() > MAX_FRAME_BYTES {
            return Err(TransportError::Io(format!(
                "a {} byte frame is over the {MAX_FRAME_BYTES} byte limit",
                frame.len()
            )));
        }
        let now = self.clock.now();
        if now.duration_since(self.window) >= SignedDuration::from_secs(1) {
            self.window = now;
            self.frames = 0;
        }
        self.frames = self.frames.saturating_add(1);
        if self.frames > MAX_FRAMES_PER_SECOND {
            return Err(TransportError::Io(format!(
                "more than {MAX_FRAMES_PER_SECOND} frames in one second"
            )));
        }
        Ok(Some(frame))
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.inner.close().await
    }
}
