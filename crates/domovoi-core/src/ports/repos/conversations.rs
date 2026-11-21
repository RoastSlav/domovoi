//! Reading Operator conversations. There is no write method: conversations change only through
//! the unit of work, by the conversation writer.

use async_trait::async_trait;

use super::{Page, Paged, StoreError};
use crate::conversation::{Conversation, Message, MessageRole};
use crate::id::{ConversationId, HostId, MessageId, OperationId, TurnId};
use crate::ports::uow::Versioned;

/// Which conversations a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationFilter {
    /// Only conversations scoped to this host.
    pub host: Option<HostId>,
    /// Only conversations with this operation attached.
    pub operation: Option<OperationId>,
}

/// A listed conversation with its version and size.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationRow {
    /// The conversation.
    pub conversation: Conversation,
    /// Its version.
    pub version: u64,
    /// How many messages it holds now.
    pub message_count: u32,
}

/// Stored Operator conversations, read-only.
#[async_trait]
pub trait ConversationRepo: Send + Sync {
    /// The conversation and its version.
    async fn get(&self, id: ConversationId) -> Result<Option<Versioned<Conversation>>, StoreError>;

    /// Newest `updated_at` first (ties by id).
    async fn list(
        &self,
        filter: &ConversationFilter,
        page: &Page,
    ) -> Result<Paged<ConversationRow>, StoreError>;

    /// All messages of one conversation, ascending `ordinal`.
    async fn messages(&self, id: ConversationId) -> Result<Vec<Message>, StoreError>;

    /// One message.
    async fn message(&self, id: MessageId) -> Result<Option<Message>, StoreError>;

    /// The message of `role` created for `turn` in `conversation`, if any.
    async fn by_turn(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        role: MessageRole,
    ) -> Result<Option<Message>, StoreError>;

    /// Up to `limit` Complete messages with `ordinal < before`, oldest first: the model's
    /// history.
    async fn recent_complete(
        &self,
        conversation: ConversationId,
        before: u32,
        limit: u16,
    ) -> Result<Vec<Message>, StoreError>;

    /// Every Pending assistant message, for startup recovery.
    async fn pending(&self) -> Result<Vec<Message>, StoreError>;
}
