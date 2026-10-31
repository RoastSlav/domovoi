//! Liveness, data the agent streams to the hub, settings pushes and errors.

use domovoi_core::{DigestFact, Signal};
use jiff::Timestamp;

use super::handshake::AgentSettings;

/// A liveness probe. Either side may send it; the other replies `pong` with
/// `reply_to` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ping {
    /// The sender's clock when sent.
    pub at: Timestamp,
}

/// The reply to `ping`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pong {
    /// The replier's clock when sent.
    pub at: Timestamp,
}

/// A batch of signals from the agent's checks or relayed local webhooks.
///
/// `batch_seq` increases by one per batch for the agent's lifetime, including batches
/// replayed from its spool after a reconnect. The hub acknowledges with `signals_ack`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Signals {
    /// This batch's sequence number.
    pub batch_seq: u64,
    /// The signals.
    pub signals: Vec<Signal>,
}

/// The hub has stored every batch up to and including `up_to_seq`; the agent may drop
/// them from its spool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignalsAck {
    /// The highest batch stored.
    pub up_to_seq: u64,
}

/// Facts for the daily digest, sent by the agent after collecting them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DigestFacts {
    /// The facts.
    pub facts: Vec<DigestFact>,
}

/// New hub-managed settings, applied by the agent immediately.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigUpdate {
    /// The complete settings; they replace the previous ones.
    pub config: AgentSettings,
}

/// A structured error, as a reply to a request or on its own. Either side may send it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ErrorMessage {
    /// A stable machine-readable code, such as `invalid_payload`.
    pub code: String,
    /// A short explanation, free of secrets.
    pub message: String,
}

/// The reply to a frame whose kind the receiver does not know.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unsupported {
    /// The kind that was not understood.
    pub kind: String,
}
