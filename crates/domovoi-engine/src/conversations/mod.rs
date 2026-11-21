//! Operator conversations: the writer of the conversation aggregate, the background turn that
//! asks the model, the live sink that streams safe text, and startup recovery.
//!
//! The Operator is read-only (INV-1, INV-9). Nothing here holds a handle that can propose,
//! approve or execute anything; the model's tools are the closed read-tool set behind
//! [`ChatModel`]. A source scan in `tests/conversations/gates.rs` keeps it that way.

mod commit;
mod handle;
pub mod journal;
mod model;
mod recover;
mod sink;
mod turn;
mod writer;

use std::sync::Arc;

use domovoi_core::actor::Actor;
use domovoi_core::conversation::{ConversationScope, MAX_QUESTION_CHARS, Message, MessageFailure};
use domovoi_core::ports::{Clock, ConversationRepo, LivePublisher, Redactor, UnitOfWork};
use domovoi_core::{ConversationId, HostId, MessageId, OperationId, TurnId};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub use handle::ConversationHandle;
pub use model::{ChatModel, ChatQuestion, DisabledModel};
pub use sink::{LIVE_MIN_INTERVAL, TurnSink, UNKNOWN_TOOL, releasable};

use crate::triage::ChatAnswer;

/// Everything the conversation service needs. No handle here can change anything but
/// conversations.
pub struct ConversationDeps {
    /// Reads of conversations and messages.
    pub conversations: Arc<dyn ConversationRepo>,
    /// The only way the service writes.
    pub uow: Arc<dyn UnitOfWork>,
    /// The model that answers questions.
    pub model: Arc<dyn ChatModel>,
    /// Where live events go.
    pub live: Arc<dyn LivePublisher>,
    /// Removes secrets from everything stored or streamed (INV-5).
    pub redactor: Arc<dyn Redactor>,
    /// Time, for timestamps and the live throttle.
    pub clock: Arc<dyn Clock>,
    /// Stops the service and cancels every running turn.
    pub shutdown: CancellationToken,
}

/// The service entry point.
pub struct ConversationService;

impl ConversationService {
    /// Runs startup recovery (every pending answer becomes failed/interrupted), then serves
    /// commands on one writer task until `deps.shutdown` is cancelled.
    pub async fn spawn(
        deps: ConversationDeps,
    ) -> Result<(ConversationHandle, JoinHandle<()>), ConversationError> {
        writer::spawn(deps).await
    }
}

/// A new conversation. The scope is always chosen by the caller.
#[derive(Debug, Clone)]
pub struct NewConversation {
    /// What the conversation is about.
    pub scope: ConversationScope,
    /// An operation to attach for read-only context.
    pub operation: Option<OperationId>,
    /// Who creates it.
    pub actor: Actor,
}

/// What the hub knows about the world for one question: built per send.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatContext {
    /// The text overview put in front of the question.
    pub overview: String,
    /// The host whose tools the model may use, only when it is connected.
    pub tool_host: Option<HostId>,
}

/// A question to ask.
#[derive(Debug, Clone)]
pub struct SendMessage {
    /// The conversation.
    pub conversation: ConversationId,
    /// The client-chosen request id that makes the send idempotent.
    pub turn: TurnId,
    /// The question text.
    pub text: String,
    /// What the model is told about the world.
    pub context: ChatContext,
    /// Who asks.
    pub actor: Actor,
}

/// A request to ask the last failed or stopped question again.
#[derive(Debug, Clone)]
pub struct RetryTurn {
    /// The conversation.
    pub conversation: ConversationId,
    /// The assistant message to retry.
    pub message: MessageId,
    /// What the model is told about the world.
    pub context: ChatContext,
    /// Who asks.
    pub actor: Actor,
}

/// The committed pair after a send or retry.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnStarted {
    /// The finished user message.
    pub user: Message,
    /// The assistant message (pending, or its current state for a replayed send).
    pub assistant: Message,
    /// Whether this was a replay of an earlier send of the same turn.
    pub replayed: bool,
}

/// Why a conversation command failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConversationError {
    /// No such conversation (or turn, or message).
    #[error("no such conversation")]
    NotFound,
    /// A different turn is still pending.
    #[error("an answer is already being written in this conversation")]
    TurnInProgress,
    /// The conversation holds `MAX_MESSAGES`.
    #[error("this conversation is full; start a new one")]
    Full,
    /// The message is not the last answer, or did not fail or stop.
    #[error("only the last answer can be retried, and only after it failed or was stopped")]
    NotRetryable,
    /// The question is empty or too long.
    #[error("the question must have 1 to {MAX_QUESTION_CHARS} characters")]
    BadQuestion,
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// The service is stopping.
    #[error("the conversation service is stopping")]
    Unavailable,
}

/// How a background turn ended, as told to the writer.
pub(crate) enum Outcome {
    /// The model answered.
    Answered(ChatAnswer),
    /// The model could not answer.
    Failed(MessageFailure, String),
}

/// The title of a conversation: the first line of the redacted question, cut.
pub(crate) fn title_of(redacted_question: &str) -> String {
    let line = redacted_question.lines().next().unwrap_or("").trim();
    cut(line, domovoi_core::conversation::MAX_TITLE_CHARS)
}

/// `text` cut to `max` characters, ending in an ellipsis when it was cut.
pub(crate) fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}
