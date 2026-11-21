//! The wire shapes of Operator conversations.

use std::collections::HashMap;

use domovoi_core::HostId;
use domovoi_core::conversation::{Conversation, ConversationScope, Message, MessageStatus};
use domovoi_core::ports::ConversationRow;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// What a conversation is about, chosen when it is created.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ScopeRequest {
    /// `global` or `host`.
    pub kind: String,
    /// The host's name; required for `host`.
    #[serde(default)]
    pub host: Option<String>,
}

/// Starts a conversation. `scope` is required: there is no implicit "all hosts".
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateConversationRequest {
    /// What the conversation is about.
    #[serde(default)]
    pub scope: Option<ScopeRequest>,
    /// An operation to attach for read-only context.
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// A question.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// A client-generated uuid: sending it again changes nothing.
    pub turn: String,
    /// The question, 1 to 1000 characters.
    pub text: String,
}

/// Attaches or detaches an operation.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetOperationRequest {
    /// The operation to attach, or `null` to detach.
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// Filters for the conversation list.
#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct ConversationListQuery {
    /// Continue after this cursor.
    pub cursor: Option<String>,
    /// The page size.
    pub limit: Option<u16>,
    /// Only conversations about this host (a host name).
    pub host: Option<String>,
    /// Only conversations with this operation attached.
    pub operation: Option<String>,
}

/// What a conversation is about, as shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ScopeDto {
    /// `global` or `host`.
    pub kind: String,
    /// The host's id, for `host`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    /// The host's name; `null` when the host is gone.
    pub host_name: Option<String>,
}

/// One tool the model used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct MessageToolDto {
    /// A read tool name, or `unknown tool`.
    pub tool: String,
    /// Whether the call failed or was refused.
    pub failed: bool,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct MessageDto {
    /// The message id.
    pub id: String,
    /// 1-based position in the conversation.
    pub ordinal: u32,
    /// `user` or `assistant`.
    pub role: String,
    /// `pending`, `complete`, `failed` or `cancelled`.
    pub status: String,
    /// The turn the question and its answer share.
    pub turn: String,
    /// The text; empty while pending, failed or cancelled.
    pub text: String,
    /// Why an answer failed or was stopped.
    pub failure: Option<String>,
    /// More about the failure.
    pub failure_detail: Option<String>,
    /// The read tools the model used.
    pub tools: Vec<MessageToolDto>,
    /// When it was created.
    pub created_at: String,
    /// When it settled.
    pub completed_at: Option<String>,
}

impl From<&Message> for MessageDto {
    fn from(message: &Message) -> Self {
        Self {
            id: message.id.to_string(),
            ordinal: message.ordinal,
            role: message.role.as_str().to_owned(),
            status: message.status.as_str().to_owned(),
            turn: message.turn.to_string(),
            text: message.text.clone(),
            failure: message.failure.map(|f| f.as_str().to_owned()),
            failure_detail: message.failure_detail.clone(),
            tools: message
                .tools
                .iter()
                .map(|t| MessageToolDto {
                    tool: t.tool.clone(),
                    failed: t.failed,
                })
                .collect(),
            created_at: message.created_at.to_string(),
            completed_at: message.completed_at.map(|t| t.to_string()),
        }
    }
}

/// A conversation without its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConversationSummaryDto {
    /// The conversation id.
    pub id: String,
    /// The first question, or empty before one is asked.
    pub title: String,
    /// What it is about.
    pub scope: ScopeDto,
    /// The attached operation.
    pub operation_id: Option<String>,
    /// How many messages it holds.
    pub message_count: u32,
    /// When it was created.
    pub created_at: String,
    /// When it last changed.
    pub updated_at: String,
    /// The stored version.
    pub version: u64,
}

impl ConversationSummaryDto {
    /// The summary of `conversation`; `names` maps host ids to their current names.
    pub(crate) fn of(
        conversation: &Conversation,
        version: u64,
        message_count: u32,
        names: &HashMap<HostId, String>,
    ) -> Self {
        let scope = match conversation.scope {
            ConversationScope::Global => ScopeDto {
                kind: "global".to_owned(),
                host_id: None,
                host_name: None,
            },
            ConversationScope::Host(id) => ScopeDto {
                kind: "host".to_owned(),
                host_id: Some(id.to_string()),
                host_name: names.get(&id).cloned(),
            },
        };
        Self {
            id: conversation.id.to_string(),
            title: conversation.title.clone(),
            scope,
            operation_id: conversation.operation.map(|o| o.to_string()),
            message_count,
            created_at: conversation.created_at.to_string(),
            updated_at: conversation.updated_at.to_string(),
            version,
        }
    }

    /// The summary of a listed row.
    pub(crate) fn of_row(row: &ConversationRow, names: &HashMap<HostId, String>) -> Self {
        Self::of(&row.conversation, row.version, row.message_count, names)
    }
}

/// The answer being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TurnInProgressDto {
    /// The turn.
    pub turn: String,
    /// The assistant message.
    pub message: String,
}

/// A conversation with its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConversationDetailDto {
    /// The summary fields.
    #[serde(flatten)]
    pub summary: ConversationSummaryDto,
    /// Every kept message, oldest first.
    pub messages: Vec<MessageDto>,
    /// The pending answer, if any.
    pub turn_in_progress: Option<TurnInProgressDto>,
    /// The journal position this was read at.
    pub as_of_seq: u64,
}

impl ConversationDetailDto {
    /// The detail of `conversation` over `messages`.
    pub(crate) fn of(
        conversation: &Conversation,
        version: u64,
        messages: &[Message],
        names: &HashMap<HostId, String>,
        as_of_seq: u64,
    ) -> Self {
        let turn_in_progress = messages
            .iter()
            .find(|m| m.status == MessageStatus::Pending)
            .map(|m| TurnInProgressDto {
                turn: m.turn.to_string(),
                message: m.id.to_string(),
            });
        let count = u32::try_from(messages.len()).unwrap_or(u32::MAX);
        Self {
            summary: ConversationSummaryDto::of(conversation, version, count, names),
            messages: messages.iter().map(MessageDto::from).collect(),
            turn_in_progress,
            as_of_seq,
        }
    }
}

/// A page of conversations, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConversationPage {
    /// The conversations.
    pub items: Vec<ConversationSummaryDto>,
    /// Pass as `cursor` for the next page.
    pub next_cursor: Option<String>,
    /// The journal position this was read at.
    pub as_of_seq: u64,
}

/// The committed question and its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SendResponse {
    /// The finished question.
    pub user: MessageDto,
    /// The answer: pending, or its current state for a repeated send.
    pub assistant: MessageDto,
    /// The journal position after the commit.
    pub as_of_seq: u64,
}

/// The answer's status after a stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CancelResponse {
    /// `cancelled` only when this call stopped it.
    pub status: String,
}
