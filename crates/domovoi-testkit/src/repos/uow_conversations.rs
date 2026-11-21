//! Applying the conversation aggregate's writes to in-memory state, with the same structural
//! rules as the SQLite tables: unique ordinals, one message per turn and role, one pending
//! answer per conversation, and messages deleted with their conversation.

use domovoi_core::ConversationId;
use domovoi_core::conversation::{Message, MessageStatus};
use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, ConversationWrite, Expectation, Expected,
    MessageGuard, Observed,
};

use super::{State, set_version, version};

fn conflict(id: ConversationId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::conversation(id),
        expected,
        found,
    }
}

fn refuse(message: &str) -> CommitError {
    CommitError::Constraint(message.to_owned())
}

/// Stores one conversation write, updating `receipt`.
pub(super) fn apply(
    state: &mut State,
    write: &ConversationWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        ConversationWrite::Conversation {
            conversation,
            expected,
        } => {
            let id = conversation.id;
            let aggregate = AggregateRef::conversation(id);
            let exists = state.conversations.contains_key(&id);
            let stored = version(state, &aggregate);
            let next = match expected {
                Expected::New if exists => {
                    return Err(conflict(id, Expectation::New, Observed::Version(stored)));
                }
                Expected::New => 1,
                Expected::Version(n) if !exists => {
                    return Err(conflict(id, Expectation::Version(*n), Observed::Missing));
                }
                Expected::Version(n) if stored != *n => {
                    return Err(conflict(
                        id,
                        Expectation::Version(*n),
                        Observed::Version(stored),
                    ));
                }
                Expected::Version(n) => n + 1,
            };
            state.conversations.insert(id, conversation.clone());
            set_version(state, aggregate.clone(), next);
            receipt.versions.push((aggregate, next));
            Ok(())
        }
        ConversationWrite::Message { message, guard } => apply_message(state, message, *guard),
        ConversationWrite::Delete { id, expected } => {
            let aggregate = AggregateRef::conversation(*id);
            if !state.conversations.contains_key(id) {
                return Err(conflict(
                    *id,
                    Expectation::Version(*expected),
                    Observed::Missing,
                ));
            }
            let stored = version(state, &aggregate);
            if stored != *expected {
                return Err(conflict(
                    *id,
                    Expectation::Version(*expected),
                    Observed::Version(stored),
                ));
            }
            state.conversations.remove(id);
            state.messages.retain(|_, m| m.conversation != *id);
            state.versions.remove(&aggregate);
            receipt.versions.push((aggregate, expected + 1));
            Ok(())
        }
    }
}

fn apply_message(
    state: &mut State,
    message: &Message,
    guard: MessageGuard,
) -> Result<(), CommitError> {
    let id = message.conversation;
    let stored = state.messages.get(&message.id).map(|m| m.status);
    match (guard, stored) {
        (MessageGuard::New, Some(status)) => {
            return Err(conflict(
                id,
                Expectation::New,
                Observed::Status(status.as_str().to_owned()),
            ));
        }
        (MessageGuard::Status(want), None) => {
            return Err(conflict(
                id,
                Expectation::Status(want.as_str().to_owned()),
                Observed::Missing,
            ));
        }
        (MessageGuard::Status(want), Some(status)) if status != want => {
            return Err(conflict(
                id,
                Expectation::Status(want.as_str().to_owned()),
                Observed::Status(status.as_str().to_owned()),
            ));
        }
        _ => {}
    }
    if !state.conversations.contains_key(&id) {
        return Err(refuse("FOREIGN KEY constraint failed"));
    }
    let others = || {
        state
            .messages
            .values()
            .filter(|m| m.conversation == id && m.id != message.id)
    };
    if others().any(|m| m.ordinal == message.ordinal) {
        return Err(refuse("UNIQUE constraint failed: messages.ordinal"));
    }
    if others().any(|m| m.turn == message.turn && m.role == message.role) {
        return Err(refuse("UNIQUE constraint failed: messages.turn_id"));
    }
    if message.status == MessageStatus::Pending
        && others().any(|m| m.status == MessageStatus::Pending)
    {
        return Err(refuse("UNIQUE constraint failed: messages.conversation_id"));
    }
    // Only the mutable columns change on a replace, as in SQL.
    let mut next = message.clone();
    if let Some(old) = state.messages.get(&message.id) {
        next.conversation = old.conversation;
        next.ordinal = old.ordinal;
        next.role = old.role;
        next.created_at = old.created_at;
    }
    state.messages.insert(next.id, next);
    Ok(())
}
