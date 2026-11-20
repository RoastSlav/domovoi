//! The state writes of the conversation aggregate, applied inside a
//! [`Transaction`](super::Transaction).

use crate::conversation::{Conversation, Message, MessageStatus};
use crate::id::ConversationId;

use super::Expected;

/// What a message write expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageGuard {
    /// No message with this id exists yet.
    New,
    /// The stored message has this status; only then is it replaced.
    Status(MessageStatus),
}

/// A write to the conversation aggregate. Built only by the conversation writer.
#[derive(Debug, Clone, PartialEq)]
pub enum ConversationWrite {
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`, the store bumps
    /// `version`) a conversation row.
    Conversation {
        /// The conversation to store.
        conversation: Conversation,
        /// What the stored row must look like.
        expected: Expected,
    },
    /// Inserts (`MessageGuard::New`) or replaces a message whose stored status is as guarded.
    Message {
        /// The message to store.
        message: Message,
        /// What the stored row must look like.
        guard: MessageGuard,
    },
    /// Deletes a conversation and, by cascade, its messages.
    Delete {
        /// The conversation.
        id: ConversationId,
        /// The version the writer read.
        expected: u64,
    },
}
