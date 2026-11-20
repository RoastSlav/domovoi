//! Startup recovery: an answer that was being written when the hub stopped is marked failed.
//!
//! A model turn is deliberately not a durable effect. Re-running a conversational answer
//! minutes after a restart would surprise the person and spend budget they did not ask for,
//! so the question stays and the answer becomes `Failed/interrupted`; only a person pressing
//! Retry asks the model again. No model call is made here.

use domovoi_core::conversation::{MessageFailure, MessageStatus};
use domovoi_core::ports::CommitError;

use super::commit::{self, Finish};
use super::{ConversationDeps, ConversationError};

/// Marks every pending assistant message failed/interrupted, one commit each.
pub(super) async fn run(deps: &ConversationDeps) -> Result<(), ConversationError> {
    let pending = deps
        .conversations
        .pending()
        .await
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    for message in pending {
        let found = deps
            .conversations
            .get(message.conversation)
            .await
            .map_err(|error| ConversationError::Store(error.to_string()))?;
        let Some(found) = found else { continue };
        let end = Finish {
            status: MessageStatus::Failed,
            text: String::new(),
            tools: Vec::new(),
            failure: Some(MessageFailure::Interrupted),
            detail: None,
        };
        let (tx, _) = commit::finish(&found.value, found.version, &message, end, deps.clock.now());
        match deps.uow.commit(tx).await {
            Ok(_) | Err(CommitError::Conflict { .. }) => {}
            Err(error) => return Err(ConversationError::Store(error.to_string())),
        }
    }
    Ok(())
}
