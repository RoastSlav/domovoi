//! The caller's side of the conversation service: commands over a channel.

use domovoi_core::actor::Actor;
use domovoi_core::conversation::{Conversation, MessageStatus};
use domovoi_core::{ConversationId, OperationId, TurnId};
use tokio::sync::{mpsc, oneshot};

use super::{ConversationError, NewConversation, Outcome, RetryTurn, SendMessage, TurnStarted};

type Reply<T> = oneshot::Sender<Result<T, ConversationError>>;

/// One command for the writer.
pub(crate) enum Command {
    Create(NewConversation, Reply<Conversation>),
    Send(SendMessage, Reply<TurnStarted>),
    Retry(RetryTurn, Reply<TurnStarted>),
    Cancel(ConversationId, TurnId, Actor, Reply<MessageStatus>),
    SetOperation(
        ConversationId,
        Option<OperationId>,
        Actor,
        Reply<Conversation>,
    ),
    Delete(ConversationId, Actor, Reply<()>),
    /// A background turn finished; nothing waits for the answer.
    Settle(ConversationId, TurnId, Outcome),
}

/// Talks to the conversation writer. Cheap to clone.
#[derive(Clone)]
pub struct ConversationHandle {
    tx: mpsc::Sender<Command>,
}

impl ConversationHandle {
    pub(super) fn new(tx: mpsc::Sender<Command>) -> Self {
        Self { tx }
    }

    async fn call<T>(
        &self,
        build: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, ConversationError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .map_err(|_| ConversationError::Unavailable)?;
        answer.await.map_err(|_| ConversationError::Unavailable)?
    }

    /// Creates a conversation with an explicit scope.
    pub async fn create(&self, new: NewConversation) -> Result<Conversation, ConversationError> {
        self.call(|reply| Command::Create(new, reply)).await
    }

    /// Commits a question and a pending answer and starts the model in the background. A
    /// second send of the same `turn` returns the stored pair with `replayed` set.
    pub async fn send(&self, cmd: SendMessage) -> Result<TurnStarted, ConversationError> {
        self.call(|reply| Command::Send(cmd, reply)).await
    }

    /// Asks the last failed or stopped question again under a new turn id.
    pub async fn retry(&self, cmd: RetryTurn) -> Result<TurnStarted, ConversationError> {
        self.call(|reply| Command::Retry(cmd, reply)).await
    }

    /// Stops a pending turn. Returns the answer's status afterwards: `Cancelled` only when
    /// this call stopped it.
    pub async fn cancel(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        actor: Actor,
    ) -> Result<MessageStatus, ConversationError> {
        self.call(|reply| Command::Cancel(conversation, turn, actor, reply))
            .await
    }

    /// Attaches or detaches an operation (read-only context for the model).
    pub async fn set_operation(
        &self,
        conversation: ConversationId,
        operation: Option<OperationId>,
        actor: Actor,
    ) -> Result<Conversation, ConversationError> {
        self.call(|reply| Command::SetOperation(conversation, operation, actor, reply))
            .await
    }

    /// Deletes a conversation and its messages; refused while an answer is pending.
    pub async fn delete(
        &self,
        conversation: ConversationId,
        actor: Actor,
    ) -> Result<(), ConversationError> {
        self.call(|reply| Command::Delete(conversation, actor, reply))
            .await
    }
}
