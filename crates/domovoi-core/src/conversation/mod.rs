//! Operator conversations: durable question and answer threads with an explicit scope.

mod message;
mod model;

pub use message::{Message, MessageFailure, MessageRole, MessageStatus, MessageTool};
pub use model::{
    Conversation, ConversationScope, MAX_FAILURE_DETAIL_CHARS, MAX_MESSAGES, MAX_QUESTION_CHARS,
    MAX_TITLE_CHARS,
};
