//! Reading Operator conversations. Writing them is the unit of work's job: see
//! `uow/conversations.rs`.

use async_trait::async_trait;
use domovoi_core::conversation::{Conversation, Message, MessageRole};
use domovoi_core::ports::{
    ConversationFilter, ConversationRepo, ConversationRow, Page, Paged, StoreError, Versioned,
};
use domovoi_core::{ConversationId, MessageId, TurnId};

use super::conversations_rows::{
    ConversationDbRow, MessageDbRow, conversation_columns, message_columns,
};
use crate::codec::{Cursor, fetch_limit, ms, paginate};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl ConversationRepo for Store {
    async fn get(&self, id: ConversationId) -> Result<Option<Versioned<Conversation>>, StoreError> {
        const SQL: &str = concat!(conversation_columns!(), " WHERE id = ?");
        let row: Option<ConversationDbRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ConversationDbRow::into_versioned).transpose()
    }

    async fn list(
        &self,
        filter: &ConversationFilter,
        page: &Page,
    ) -> Result<Paged<ConversationRow>, StoreError> {
        const SQL: &str = concat!(
            conversation_columns!(),
            " WHERE (?1 IS NULL OR host_id = ?1) AND (?2 IS NULL OR operation_id = ?2) \
             AND (?3 IS NULL OR updated_at < ?3 OR (updated_at = ?3 AND id < ?4)) \
             ORDER BY updated_at DESC, id DESC LIMIT ?5"
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<ConversationDbRow> = sqlx::query_as(SQL)
            .bind(filter.host.map(|h| h.to_string()))
            .bind(filter.operation.map(|o| o.to_string()))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let rows = rows
            .into_iter()
            .map(ConversationDbRow::into_row)
            .collect::<Result<Vec<_>, _>>()?;
        let (items, next_cursor) = paginate(rows, limit, |row| Cursor {
            key: ms(row.conversation.updated_at),
            id: row.conversation.id.to_string(),
        });
        Ok(Paged { items, next_cursor })
    }

    async fn messages(&self, id: ConversationId) -> Result<Vec<Message>, StoreError> {
        const SQL: &str = concat!(
            message_columns!(),
            " WHERE conversation_id = ? ORDER BY ordinal"
        );
        let rows: Vec<MessageDbRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(MessageDbRow::into_message).collect()
    }

    async fn message(&self, id: MessageId) -> Result<Option<Message>, StoreError> {
        const SQL: &str = concat!(message_columns!(), " WHERE id = ?");
        let row: Option<MessageDbRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(MessageDbRow::into_message).transpose()
    }

    async fn by_turn(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        role: MessageRole,
    ) -> Result<Option<Message>, StoreError> {
        const SQL: &str = concat!(
            message_columns!(),
            " WHERE conversation_id = ? AND turn_id = ? AND role = ?"
        );
        let row: Option<MessageDbRow> = sqlx::query_as(SQL)
            .bind(conversation.to_string())
            .bind(turn.to_string())
            .bind(role.as_str())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(MessageDbRow::into_message).transpose()
    }

    async fn recent_complete(
        &self,
        conversation: ConversationId,
        before: u32,
        limit: u16,
    ) -> Result<Vec<Message>, StoreError> {
        // The newest `limit` below `before`, returned oldest first.
        const SQL: &str = concat!(
            "SELECT * FROM (",
            message_columns!(),
            " WHERE conversation_id = ? AND status = 'complete' AND ordinal < ? \
             ORDER BY ordinal DESC LIMIT ?) ORDER BY ordinal"
        );
        let rows: Vec<MessageDbRow> = sqlx::query_as(SQL)
            .bind(conversation.to_string())
            .bind(i64::from(before))
            .bind(i64::from(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(MessageDbRow::into_message).collect()
    }

    async fn pending(&self) -> Result<Vec<Message>, StoreError> {
        const SQL: &str = concat!(
            message_columns!(),
            " WHERE status = 'pending' AND role = 'assistant' ORDER BY created_at, ordinal"
        );
        let rows: Vec<MessageDbRow> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(MessageDbRow::into_message).collect()
    }
}
