//! Journal payload builders and drafts for the conversation kinds (payload version 1).
//!
//! No payload and no audit detail ever carries message text, a title or a failure detail.

use domovoi_core::actor::{Actor, SYSTEM_OPERATOR};
use domovoi_core::conversation::{Conversation, ConversationScope, Message};
use domovoi_core::ports::{AggregateRef, JournalDraft, JournalKind};
use domovoi_core::{ConversationId, OperationId};
use jiff::Timestamp;
use serde_json::{Value, json};

/// The payload version every conversation journal entry carries.
pub const PAYLOAD_V: u32 = 1;

/// The actor of every decision the service makes itself.
#[must_use]
pub fn operator() -> Actor {
    Actor::System(SYSTEM_OPERATOR.to_owned())
}

fn draft(
    id: ConversationId,
    kind: JournalKind,
    payload: Value,
    actor: Actor,
    at: Timestamp,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::conversation(id),
        kind,
        payload_v: PAYLOAD_V,
        payload,
        actor,
        occurred_at: at,
    }
}

/// The `conversation.created` row.
#[must_use]
pub fn created(conversation: &Conversation, actor: Actor, at: Timestamp) -> JournalDraft {
    let mut payload = json!({
        "conversation": conversation.id.to_string(),
        "scope": conversation.scope.kind(),
    });
    if let ConversationScope::Host(host) = conversation.scope {
        payload["host"] = json!(host.to_string());
    }
    if let Some(operation) = conversation.operation {
        payload["operation"] = json!(operation.to_string());
    }
    draft(
        conversation.id,
        JournalKind::ConversationCreated,
        payload,
        actor,
        at,
    )
}

/// The `conversation.message` row for a message that was created or changed status.
#[must_use]
pub fn message(message: &Message, actor: Actor, at: Timestamp) -> JournalDraft {
    let mut payload = json!({
        "conversation": message.conversation.to_string(),
        "message": message.id.to_string(),
        "ordinal": message.ordinal,
        "role": message.role.as_str(),
        "status": message.status.as_str(),
        "turn": message.turn.to_string(),
    });
    if let Some(failure) = message.failure {
        payload["failure"] = json!(failure.as_str());
    }
    draft(
        message.conversation,
        JournalKind::ConversationMessage,
        payload,
        actor,
        at,
    )
}

/// The `conversation.updated` row for an attached or detached operation.
#[must_use]
pub fn operation_changed(
    id: ConversationId,
    operation: Option<OperationId>,
    actor: Actor,
    at: Timestamp,
) -> JournalDraft {
    let mut payload = json!({
        "conversation": id.to_string(),
        "change": if operation.is_some() { "operation_attached" } else { "operation_detached" },
    });
    if let Some(operation) = operation {
        payload["operation"] = json!(operation.to_string());
    }
    draft(id, JournalKind::ConversationUpdated, payload, actor, at)
}

/// The `conversation.deleted` row.
#[must_use]
pub fn deleted(id: ConversationId, actor: Actor, at: Timestamp) -> JournalDraft {
    draft(
        id,
        JournalKind::ConversationDeleted,
        json!({ "conversation": id.to_string() }),
        actor,
        at,
    )
}
