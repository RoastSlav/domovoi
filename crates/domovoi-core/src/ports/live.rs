//! Ephemeral live events of an Operator answer being written. Never stored, never replayed.

use crate::id::{ConversationId, MessageId, TurnId};
use crate::ports::llm::ToolPhase;

/// Publishes ephemeral live events to whoever watches a conversation. Never blocks, never
/// fails, never stores.
pub trait LivePublisher: Send + Sync {
    /// Offers `event` to the watchers of `conversation`; it is dropped when nobody listens.
    fn publish(&self, conversation: ConversationId, event: LiveEvent);
}

/// How a turn ended, as told to watchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveEnd {
    /// The answer was committed.
    Complete,
    /// The answer failed and the failure was committed.
    Failed,
    /// The person stopped the turn.
    Cancelled,
    /// A result arrived for a turn that was no longer pending and was not committed.
    Discarded,
}

/// One live event of a turn.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    /// The model call begins.
    Start {
        /// The turn.
        turn: TurnId,
        /// The assistant message being written.
        message: MessageId,
    },
    /// The whole safe text of the current attempt so far (a snapshot, not a delta).
    Text {
        /// The turn.
        turn: TurnId,
        /// Starts at 1 and grows when the model starts over.
        attempt: u32,
        /// The safe text.
        text: String,
    },
    /// A read tool started or finished.
    Tool {
        /// The turn.
        turn: TurnId,
        /// A closed tool name or `unknown tool`.
        tool: String,
        /// Started or finished.
        phase: ToolPhase,
        /// Whether the call failed.
        failed: bool,
    },
    /// The turn is over.
    End {
        /// The turn.
        turn: TurnId,
        /// How it ended.
        outcome: LiveEnd,
    },
}
