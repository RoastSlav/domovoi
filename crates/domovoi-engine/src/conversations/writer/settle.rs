//! Settling a finished turn: commit it only while it is still the pending one.

use domovoi_core::conversation::{MessageFailure, MessageRole, MessageStatus, MessageTool};
use domovoi_core::ports::{LiveEnd, LiveEvent};
use domovoi_core::{ConversationId, TurnId};

use super::{ConversationError, Finish, Outcome, Step, Writer, commit};

impl Writer {
    /// Commits a finished turn only when it is still the pending one; anything else is a stale
    /// result and is discarded.
    pub(super) async fn settle(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        outcome: Outcome,
    ) {
        let end = match &outcome {
            Outcome::Answered(_) => LiveEnd::Complete,
            Outcome::Failed(..) => LiveEnd::Failed,
        };
        let result = match self.settle_once(conversation, turn, &outcome).await {
            Err(Step::Fail(ConversationError::Store(_))) if end == LiveEnd::Complete => {
                let fallback = Outcome::Failed(
                    MessageFailure::Internal,
                    "the answer could not be stored".to_owned(),
                );
                self.settle_once(conversation, turn, &fallback)
                    .await
                    .map(|committed| committed.map(|_| LiveEnd::Failed))
            }
            other => other.map(|committed| committed.map(|()| end)),
        };
        let shown = match result {
            Ok(Some(end)) => end,
            Ok(None) => LiveEnd::Discarded,
            Err(_) => {
                tracing::error!(%conversation, %turn, "a finished answer could not be stored");
                LiveEnd::Failed
            }
        };
        self.deps.live.publish(
            conversation,
            LiveEvent::End {
                turn,
                outcome: shown,
            },
        );
    }

    async fn settle_once(
        &self,
        conversation_id: ConversationId,
        turn: TurnId,
        outcome: &Outcome,
    ) -> Result<Option<()>, Step> {
        for _ in 0..2 {
            let (conversation, version) = match self.load(conversation_id).await {
                Ok(found) => found,
                Err(Step::Fail(ConversationError::NotFound)) => return Ok(None),
                Err(other) => return Err(other),
            };
            let messages = self.messages(conversation_id).await?;
            let Some(assistant) = messages
                .iter()
                .find(|m| m.role == MessageRole::Assistant && m.turn == turn)
                .filter(|m| m.status == MessageStatus::Pending)
            else {
                return Ok(None);
            };
            let end = self.finish_of(outcome);
            let (tx, _) = commit::finish(
                &conversation,
                version,
                assistant,
                end,
                self.deps.clock.now(),
            );
            match self.commit(tx).await {
                Ok(()) => return Ok(Some(())),
                Err(Step::Conflict) => {}
                Err(other) => return Err(other),
            }
        }
        Err(Step::Conflict)
    }

    fn finish_of(&self, outcome: &Outcome) -> Finish {
        match outcome {
            Outcome::Answered(answer) => Finish {
                status: MessageStatus::Complete,
                text: self.deps.redactor.redact(&answer.text),
                tools: answer
                    .tools
                    .iter()
                    .map(|tool| MessageTool {
                        tool: tool.tool.clone(),
                        failed: tool.failed || tool.refused,
                    })
                    .collect(),
                failure: None,
                detail: None,
            },
            Outcome::Failed(code, detail) => Finish {
                status: MessageStatus::Failed,
                text: String::new(),
                tools: Vec::new(),
                failure: Some(*code),
                detail: Some(detail.clone()),
            },
        }
    }
}
