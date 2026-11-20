//! The in-memory conversation repository: reads only. Writes arrive through the unit of work.

use async_trait::async_trait;
use domovoi_core::conversation::{Conversation, Message, MessageRole, MessageStatus};
use domovoi_core::ports::{
    AggregateRef, ConversationFilter, ConversationRepo, ConversationRow, Page, Paged, StoreError,
    Versioned,
};
use domovoi_core::{ConversationId, MessageId, TurnId};

use super::{InMemoryStore, paginate, version};

#[async_trait]
impl ConversationRepo for InMemoryStore {
    async fn get(&self, id: ConversationId) -> Result<Option<Versioned<Conversation>>, StoreError> {
        let state = self.begin()?;
        Ok(state.conversations.get(&id).map(|c| Versioned {
            value: c.clone(),
            version: version(&state, &AggregateRef::conversation(id)),
        }))
    }

    async fn list(
        &self,
        filter: &ConversationFilter,
        page: &Page,
    ) -> Result<Paged<ConversationRow>, StoreError> {
        let state = self.begin()?;
        let items = state
            .conversations
            .values()
            .filter(|c| {
                filter.host.is_none_or(|h| c.scope.host() == Some(h))
                    && filter.operation.is_none_or(|o| c.operation == Some(o))
            })
            .map(|c| {
                let count = state
                    .messages
                    .values()
                    .filter(|m| m.conversation == c.id)
                    .count();
                (
                    (c.updated_at.as_nanosecond(), c.id.to_string()),
                    ConversationRow {
                        conversation: c.clone(),
                        version: version(&state, &AggregateRef::conversation(c.id)),
                        message_count: u32::try_from(count).unwrap_or(u32::MAX),
                    },
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn messages(&self, id: ConversationId) -> Result<Vec<Message>, StoreError> {
        let state = self.begin()?;
        let mut all: Vec<Message> = state
            .messages
            .values()
            .filter(|m| m.conversation == id)
            .cloned()
            .collect();
        all.sort_by_key(|m| m.ordinal);
        Ok(all)
    }

    async fn message(&self, id: MessageId) -> Result<Option<Message>, StoreError> {
        Ok(self.begin()?.messages.get(&id).cloned())
    }

    async fn by_turn(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        role: MessageRole,
    ) -> Result<Option<Message>, StoreError> {
        Ok(self
            .begin()?
            .messages
            .values()
            .find(|m| m.conversation == conversation && m.turn == turn && m.role == role)
            .cloned())
    }

    async fn recent_complete(
        &self,
        conversation: ConversationId,
        before: u32,
        limit: u16,
    ) -> Result<Vec<Message>, StoreError> {
        let state = self.begin()?;
        let mut all: Vec<Message> = state
            .messages
            .values()
            .filter(|m| {
                m.conversation == conversation
                    && m.status == MessageStatus::Complete
                    && m.ordinal < before
            })
            .cloned()
            .collect();
        all.sort_by_key(|m| m.ordinal);
        let skip = all.len().saturating_sub(usize::from(limit));
        Ok(all.split_off(skip))
    }

    async fn pending(&self) -> Result<Vec<Message>, StoreError> {
        let state = self.begin()?;
        let mut all: Vec<Message> = state
            .messages
            .values()
            .filter(|m| m.status == MessageStatus::Pending && m.role == MessageRole::Assistant)
            .cloned()
            .collect();
        all.sort_by_key(|m| (m.created_at, m.ordinal));
        Ok(all)
    }
}
