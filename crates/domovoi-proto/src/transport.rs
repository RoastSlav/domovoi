//! A bidirectional stream of text frames.

use async_trait::async_trait;

/// Why a transport operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The peer closed the connection.
    #[error("connection closed")]
    Closed,
    /// The underlying connection failed.
    #[error("transport error: {0}")]
    Io(String),
}

/// One end of an agent link, carrying encoded frames.
///
/// The WebSocket transport lives in the runtimes; single-box mode uses an in-memory
/// pair. Both carry exactly the frames [`crate::encode`] produces.
#[async_trait]
pub trait Transport: Send {
    /// Sends one frame.
    async fn send(&mut self, frame: String) -> Result<(), TransportError>;
    /// Receives the next frame, or `None` once the peer has closed cleanly.
    async fn recv(&mut self) -> Result<Option<String>, TransportError>;
}
