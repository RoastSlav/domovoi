//! Rows of the conversation tables and how they decode into domain types.
//!
//! Stored names (scope, role, status, failure) are validated here, in Rust, when read: an
//! unknown one is a backend error that names the value.

use domovoi_core::conversation::{
    Conversation, ConversationScope, Message, MessageFailure, MessageRole, MessageStatus,
    MessageTool,
};
use domovoi_core::ports::{ConversationRow, StoreError, Versioned};

use crate::codec::{from_json, from_ms, from_ms_opt, parse, uint};
use crate::error::corrupt;

/// The columns of a conversation row, in [`ConversationDbRow`] order.
macro_rules! conversation_columns {
    () => {
        "SELECT id, version, title, scope_kind, host_id, operation_id, last_ordinal, \
         created_at, updated_at, \
         (SELECT COUNT(*) FROM messages WHERE messages.conversation_id = conversations.id) \
         AS message_count FROM conversations"
    };
}
pub(super) use conversation_columns;

/// The columns of a message row, in [`MessageDbRow`] order.
macro_rules! message_columns {
    () => {
        "SELECT id, conversation_id, ordinal, role, status, turn_id, text, failure, \
         failure_detail, tools, created_at, completed_at FROM messages"
    };
}
pub(super) use message_columns;

/// One row of `conversations`, with its message count.
#[derive(sqlx::FromRow)]
pub(super) struct ConversationDbRow {
    id: String,
    version: i64,
    title: String,
    scope_kind: String,
    host_id: Option<String>,
    operation_id: Option<String>,
    last_ordinal: i64,
    created_at: i64,
    updated_at: i64,
    message_count: i64,
}

impl ConversationDbRow {
    pub(super) fn into_row(self) -> Result<ConversationRow, StoreError> {
        let host = self
            .host_id
            .as_deref()
            .map(|h| parse(h, "conversation host"))
            .transpose()?;
        let scope = ConversationScope::from_parts(&self.scope_kind, host)
            .ok_or_else(|| corrupt("conversation scope", &self.scope_kind))?;
        Ok(ConversationRow {
            conversation: Conversation {
                id: parse(&self.id, "conversation id")?,
                title: self.title,
                scope,
                operation: self
                    .operation_id
                    .as_deref()
                    .map(|o| parse(o, "conversation operation"))
                    .transpose()?,
                last_ordinal: u32::try_from(self.last_ordinal)
                    .map_err(|e| corrupt("conversation ordinal", e))?,
                created_at: from_ms(self.created_at)?,
                updated_at: from_ms(self.updated_at)?,
            },
            version: uint(self.version),
            message_count: u32::try_from(self.message_count).unwrap_or(u32::MAX),
        })
    }

    pub(super) fn into_versioned(self) -> Result<Versioned<Conversation>, StoreError> {
        let row = self.into_row()?;
        Ok(Versioned {
            value: row.conversation,
            version: row.version,
        })
    }
}

/// One row of `messages`.
#[derive(sqlx::FromRow)]
pub(super) struct MessageDbRow {
    id: String,
    conversation_id: String,
    ordinal: i64,
    role: String,
    status: String,
    turn_id: String,
    text: String,
    failure: Option<String>,
    failure_detail: Option<String>,
    tools: String,
    created_at: i64,
    completed_at: Option<i64>,
}

impl MessageDbRow {
    pub(super) fn into_message(self) -> Result<Message, StoreError> {
        let role =
            MessageRole::parse(&self.role).ok_or_else(|| corrupt("message role", &self.role))?;
        let status = MessageStatus::parse(&self.status)
            .ok_or_else(|| corrupt("message status", &self.status))?;
        let failure = self
            .failure
            .as_deref()
            .map(|f| MessageFailure::parse(f).ok_or_else(|| corrupt("message failure", f)))
            .transpose()?;
        let tools: Vec<MessageTool> = from_json(&self.tools, "message tools")?;
        Ok(Message {
            id: parse(&self.id, "message id")?,
            conversation: parse(&self.conversation_id, "conversation id")?,
            ordinal: u32::try_from(self.ordinal).map_err(|e| corrupt("message ordinal", e))?,
            role,
            status,
            turn: parse(&self.turn_id, "turn id")?,
            text: self.text,
            failure,
            failure_detail: self.failure_detail,
            tools,
            created_at: from_ms(self.created_at)?,
            completed_at: from_ms_opt(self.completed_at)?,
        })
    }
}
