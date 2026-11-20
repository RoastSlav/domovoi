//! The Operator conversation use cases: every command on a conversation goes through here
//! with the person who asked.
//!
//! The Operator is read-only: nothing here proposes, approves or runs anything.

use domovoi_core::conversation::{Conversation, ConversationScope, MessageStatus};
use domovoi_core::{ConversationId, MessageId, OperationId, TurnId};
use domovoi_engine::conversations::{
    ChatContext, ConversationError, ConversationHandle, NewConversation, RetryTurn, SendMessage,
    TurnStarted,
};

use super::Caller;

/// Creates, asks, stops, retries, attaches and deletes Operator conversations.
#[derive(Clone)]
pub struct ConversationCommands {
    handle: ConversationHandle,
}

impl ConversationCommands {
    /// The commands over the conversation writer.
    #[must_use]
    pub fn new(handle: ConversationHandle) -> Self {
        Self { handle }
    }

    /// Starts a conversation with an explicit scope.
    pub async fn create(
        &self,
        caller: &Caller,
        scope: ConversationScope,
        operation: Option<OperationId>,
    ) -> Result<Conversation, ConversationError> {
        self.handle
            .create(NewConversation {
                scope,
                operation,
                actor: caller.actor(),
            })
            .await
    }

    /// Commits a question and a pending answer; the answer is written in the background.
    pub async fn send(
        &self,
        caller: &Caller,
        conversation: ConversationId,
        turn: TurnId,
        text: String,
        context: ChatContext,
    ) -> Result<TurnStarted, ConversationError> {
        self.handle
            .send(SendMessage {
                conversation,
                turn,
                text,
                context,
                actor: caller.actor(),
            })
            .await
    }

    /// Asks the last failed or stopped question again under a new turn.
    pub async fn retry(
        &self,
        caller: &Caller,
        conversation: ConversationId,
        message: MessageId,
        context: ChatContext,
    ) -> Result<TurnStarted, ConversationError> {
        self.handle
            .retry(RetryTurn {
                conversation,
                message,
                context,
                actor: caller.actor(),
            })
            .await
    }

    /// Stops a running turn; returns the answer's status afterwards.
    pub async fn cancel(
        &self,
        caller: &Caller,
        conversation: ConversationId,
        turn: TurnId,
    ) -> Result<MessageStatus, ConversationError> {
        self.handle.cancel(conversation, turn, caller.actor()).await
    }

    /// Attaches or detaches an operation (read-only context).
    pub async fn set_operation(
        &self,
        caller: &Caller,
        conversation: ConversationId,
        operation: Option<OperationId>,
    ) -> Result<Conversation, ConversationError> {
        self.handle
            .set_operation(conversation, operation, caller.actor())
            .await
    }

    /// Deletes a conversation and its messages.
    pub async fn delete(
        &self,
        caller: &Caller,
        conversation: ConversationId,
    ) -> Result<(), ConversationError> {
        self.handle.delete(conversation, caller.actor()).await
    }
}
