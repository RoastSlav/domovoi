//! The pure part of the writer: every command's transaction, built from fresh state.

use domovoi_core::actor::Actor;
use domovoi_core::conversation::{
    Conversation, Message, MessageFailure, MessageStatus, MessageTool,
};
use domovoi_core::ports::{
    AuditDraft, ConversationWrite, Expected, MessageGuard, StateWrite, Transaction,
};
use domovoi_core::{ConversationId, OperationId, TurnId};
use jiff::Timestamp;
use serde_json::json;

use super::{journal, title_of};

fn bump(conversation: &Conversation, now: Timestamp) -> Conversation {
    let mut next = conversation.clone();
    next.updated_at = now;
    next
}

fn conversation_write(conversation: Conversation, version: u64) -> StateWrite {
    StateWrite::Conversation(ConversationWrite::Conversation {
        conversation,
        expected: Expected::Version(version),
    })
}

fn message_write(message: &Message, guard: MessageGuard) -> StateWrite {
    StateWrite::Conversation(ConversationWrite::Message {
        message: message.clone(),
        guard,
    })
}

/// A new conversation.
pub(super) fn create(conversation: &Conversation, actor: Actor) -> Transaction {
    let mut tx =
        Transaction::new().write(StateWrite::Conversation(ConversationWrite::Conversation {
            conversation: conversation.clone(),
            expected: Expected::New,
        }));
    tx.journal.push(journal::created(
        conversation,
        actor,
        conversation.created_at,
    ));
    tx
}

/// A question and its pending answer, in one transaction.
pub(super) struct Sent {
    pub tx: Transaction,
    pub user: Message,
    pub assistant: Message,
}

/// Commits the question `redacted` (already redacted) and a pending answer.
pub(super) fn send(
    conversation: &Conversation,
    version: u64,
    redacted: &str,
    turn: TurnId,
    now: Timestamp,
    actor: Actor,
) -> Sent {
    let mut next = bump(conversation, now);
    if next.title.is_empty() {
        next.title = title_of(redacted);
    }
    let user_ordinal = conversation.last_ordinal + 1;
    next.last_ordinal = user_ordinal + 1;
    let user = Message::user(
        conversation.id,
        user_ordinal,
        turn,
        redacted.to_owned(),
        now,
    );
    let assistant = Message::pending_answer(conversation.id, user_ordinal + 1, turn, now);
    let mut tx = Transaction::new()
        .write(conversation_write(next, version))
        .write(message_write(&user, MessageGuard::New))
        .write(message_write(&assistant, MessageGuard::New));
    tx.journal.push(journal::message(&user, actor, now));
    tx.journal
        .push(journal::message(&assistant, journal::operator(), now));
    Sent {
        tx,
        user,
        assistant,
    }
}

/// How a pending answer ends.
pub(super) struct Finish {
    pub status: MessageStatus,
    pub text: String,
    pub tools: Vec<MessageTool>,
    pub failure: Option<MessageFailure>,
    pub detail: Option<String>,
}

/// Settles a pending answer (answered, failed, stopped or interrupted).
pub(super) fn finish(
    conversation: &Conversation,
    version: u64,
    assistant: &Message,
    end: Finish,
    now: Timestamp,
) -> (Transaction, Message) {
    let mut done = assistant.clone();
    done.status = end.status;
    done.text = end.text;
    done.tools = end.tools;
    done.failure = end.failure;
    done.failure_detail = end.detail;
    done.completed_at = Some(now);
    let mut tx = Transaction::new()
        .write(message_write(
            &done,
            MessageGuard::Status(MessageStatus::Pending),
        ))
        .write(conversation_write(bump(conversation, now), version));
    tx.journal
        .push(journal::message(&done, journal::operator(), now));
    (tx, done)
}

/// Turns a failed or stopped answer back into a pending one with a new turn.
pub(super) fn retry(
    conversation: &Conversation,
    version: u64,
    assistant: &Message,
    turn: TurnId,
    now: Timestamp,
) -> (Transaction, Message) {
    let mut again = assistant.clone();
    let before = assistant.status;
    again.status = MessageStatus::Pending;
    again.turn = turn;
    again.text.clear();
    again.tools.clear();
    again.failure = None;
    again.failure_detail = None;
    again.completed_at = None;
    let mut tx = Transaction::new()
        .write(message_write(&again, MessageGuard::Status(before)))
        .write(conversation_write(bump(conversation, now), version));
    tx.journal
        .push(journal::message(&again, journal::operator(), now));
    (tx, again)
}

/// Attaches or detaches an operation.
pub(super) fn set_operation(
    conversation: &Conversation,
    version: u64,
    operation: Option<OperationId>,
    now: Timestamp,
    actor: Actor,
) -> (Transaction, Conversation) {
    let mut next = bump(conversation, now);
    next.operation = operation;
    let mut tx = Transaction::new().write(conversation_write(next.clone(), version));
    tx.journal.push(journal::operation_changed(
        conversation.id,
        operation,
        actor,
        now,
    ));
    (tx, next)
}

/// Deletes a conversation and its messages, with an audit row because data is destroyed.
pub(super) fn delete(
    id: ConversationId,
    version: u64,
    messages: usize,
    now: Timestamp,
    actor: Actor,
) -> Transaction {
    let mut tx = Transaction::new().write(StateWrite::Conversation(ConversationWrite::Delete {
        id,
        expected: version,
    }));
    tx.journal.push(journal::deleted(id, actor.clone(), now));
    tx.audit.push(AuditDraft::new(
        actor,
        now,
        "conversation.deleted".to_owned(),
        format!("conversation:{id}"),
        json!({ "messages": messages }),
    ));
    tx
}
