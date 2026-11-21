//! Applying the conversation aggregate's writes on one SQLite connection.

use domovoi_core::conversation::{Conversation, Message};
use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, ConversationWrite, Expectation, Expected,
    MessageGuard, Observed,
};
use domovoi_core::{ConversationId, MessageId};
use sqlx::SqliteConnection;

use crate::codec::{int, json, ms, uint};

use super::{backend, map_store};

fn conflict(id: ConversationId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::conversation(id),
        expected,
        found,
    }
}

/// Applies one conversation write.
pub(super) async fn apply(
    conn: &mut SqliteConnection,
    write: &ConversationWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        ConversationWrite::Conversation {
            conversation,
            expected,
        } => apply_conversation(conn, conversation, *expected, receipt).await,
        ConversationWrite::Message { message, guard } => apply_message(conn, message, *guard).await,
        ConversationWrite::Delete { id, expected } => {
            apply_delete(conn, *id, *expected, receipt).await
        }
    }
}

async fn stored_version(
    conn: &mut SqliteConnection,
    id: ConversationId,
) -> Result<Option<u64>, CommitError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT version FROM conversations WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(row.map(|(version,)| uint(version)))
}

async fn apply_conversation(
    conn: &mut SqliteConnection,
    conversation: &Conversation,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let id = conversation.id;
    let stored = stored_version(conn, id).await?;
    let aggregate = AggregateRef::conversation(id);
    match (expected, stored) {
        (Expected::New, Some(v)) => Err(conflict(id, Expectation::New, Observed::Version(v))),
        (Expected::New, None) => {
            insert_conversation(conn, conversation).await?;
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        (Expected::Version(n), None) => {
            Err(conflict(id, Expectation::Version(n), Observed::Missing))
        }
        (Expected::Version(n), Some(v)) if v != n => {
            Err(conflict(id, Expectation::Version(n), Observed::Version(v)))
        }
        (Expected::Version(n), Some(_)) => {
            update_conversation(conn, conversation, n).await?;
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}

async fn insert_conversation(
    conn: &mut SqliteConnection,
    c: &Conversation,
) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO conversations (id, version, title, scope_kind, host_id, \
        operation_id, last_ordinal, created_at, updated_at) VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?)";
    sqlx::query(SQL)
        .bind(c.id.to_string())
        .bind(&c.title)
        .bind(c.scope.kind())
        .bind(c.scope.host().map(|h| h.to_string()))
        .bind(c.operation.map(|o| o.to_string()))
        .bind(i64::from(c.last_ordinal))
        .bind(ms(c.created_at))
        .bind(ms(c.updated_at))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn update_conversation(
    conn: &mut SqliteConnection,
    c: &Conversation,
    version: u64,
) -> Result<(), CommitError> {
    const SQL: &str = "UPDATE conversations SET title = ?, scope_kind = ?, host_id = ?, \
        operation_id = ?, last_ordinal = ?, created_at = ?, updated_at = ?, \
        version = version + 1 WHERE id = ? AND version = ?";
    sqlx::query(SQL)
        .bind(&c.title)
        .bind(c.scope.kind())
        .bind(c.scope.host().map(|h| h.to_string()))
        .bind(c.operation.map(|o| o.to_string()))
        .bind(i64::from(c.last_ordinal))
        .bind(ms(c.created_at))
        .bind(ms(c.updated_at))
        .bind(c.id.to_string())
        .bind(int(version))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn stored_status(
    conn: &mut SqliteConnection,
    id: MessageId,
) -> Result<Option<String>, CommitError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT status FROM messages WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(row.map(|(status,)| status))
}

async fn apply_message(
    conn: &mut SqliteConnection,
    message: &Message,
    guard: MessageGuard,
) -> Result<(), CommitError> {
    let stored = stored_status(conn, message.id).await?;
    let id = message.conversation;
    match (guard, stored) {
        (MessageGuard::New, None) => insert_message(conn, message).await,
        (MessageGuard::New, Some(status)) => {
            Err(conflict(id, Expectation::New, Observed::Status(status)))
        }
        (MessageGuard::Status(want), None) => Err(conflict(
            id,
            Expectation::Status(want.as_str().to_owned()),
            Observed::Missing,
        )),
        (MessageGuard::Status(want), Some(status)) if status != want.as_str() => Err(conflict(
            id,
            Expectation::Status(want.as_str().to_owned()),
            Observed::Status(status),
        )),
        (MessageGuard::Status(_), Some(_)) => replace_message(conn, message).await,
    }
}

async fn insert_message(conn: &mut SqliteConnection, m: &Message) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO messages (id, conversation_id, ordinal, role, status, \
        turn_id, text, failure, failure_detail, tools, created_at, completed_at) \
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    sqlx::query(SQL)
        .bind(m.id.to_string())
        .bind(m.conversation.to_string())
        .bind(i64::from(m.ordinal))
        .bind(m.role.as_str())
        .bind(m.status.as_str())
        .bind(m.turn.to_string())
        .bind(&m.text)
        .bind(m.failure.map(|f| f.as_str()))
        .bind(&m.failure_detail)
        .bind(json(&m.tools).map_err(map_store)?)
        .bind(ms(m.created_at))
        .bind(m.completed_at.map(ms))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn replace_message(conn: &mut SqliteConnection, m: &Message) -> Result<(), CommitError> {
    const SQL: &str = "UPDATE messages SET status = ?, turn_id = ?, text = ?, failure = ?, \
        failure_detail = ?, tools = ?, completed_at = ? WHERE id = ?";
    sqlx::query(SQL)
        .bind(m.status.as_str())
        .bind(m.turn.to_string())
        .bind(&m.text)
        .bind(m.failure.map(|f| f.as_str()))
        .bind(&m.failure_detail)
        .bind(json(&m.tools).map_err(map_store)?)
        .bind(m.completed_at.map(ms))
        .bind(m.id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn apply_delete(
    conn: &mut SqliteConnection,
    id: ConversationId,
    expected: u64,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match stored_version(conn, id).await? {
        None => Err(conflict(
            id,
            Expectation::Version(expected),
            Observed::Missing,
        )),
        Some(v) if v != expected => Err(conflict(
            id,
            Expectation::Version(expected),
            Observed::Version(v),
        )),
        Some(_) => {
            sqlx::query("DELETE FROM conversations WHERE id = ?")
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            receipt
                .versions
                .push((AggregateRef::conversation(id), expected + 1));
            Ok(())
        }
    }
}
