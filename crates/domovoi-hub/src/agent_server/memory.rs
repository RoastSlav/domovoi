//! An in-memory transport pair, for an agent running inside the hub process.

use async_trait::async_trait;
use domovoi_proto::{Transport, TransportError};
use tokio::sync::mpsc;

/// How many frames may wait in each direction.
const BUFFER: usize = 256;

/// One end of an in-memory connection. Dropping it closes the connection for the other.
pub struct MemoryTransport {
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<String>,
}

impl MemoryTransport {
    /// Two connected ends: what one sends, the other receives.
    #[must_use]
    pub fn pair() -> (Self, Self) {
        let (a_tx, b_rx) = mpsc::channel(BUFFER);
        let (b_tx, a_rx) = mpsc::channel(BUFFER);
        (Self { tx: a_tx, rx: a_rx }, Self { tx: b_tx, rx: b_rx })
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        self.tx
            .send(frame)
            .await
            .map_err(|_| TransportError::Closed)
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        Ok(self.rx.recv().await)
    }
}
