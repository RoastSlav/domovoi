//! The model the Operator asks, behind a port so the service never sees the triage internals.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::ports::AnswerSink;

use crate::triage::{ChatAnswer, ChatTurn, TriageError, TriageHandle};

/// One question for the model: the same input the general chat always had.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatQuestion {
    /// The host whose read tools the model may use; `None` offers no tools.
    pub host: Option<HostId>,
    /// The text overview of hosts and incidents put in front of the question.
    pub overview: String,
    /// The earlier complete exchanges, oldest first.
    pub history: Vec<ChatTurn>,
    /// The new question.
    pub question: String,
}

/// Answers Operator questions. Read-only by construction: the only implementation that
/// reaches a model is the triage chat, whose tools are the closed read-tool set.
#[async_trait]
pub trait ChatModel: Send + Sync {
    /// Answers `question`, writing what the model produces to `sink` while it works.
    async fn answer(
        &self,
        question: ChatQuestion,
        sink: Arc<dyn AnswerSink>,
    ) -> Result<ChatAnswer, TriageError>;
}

#[async_trait]
impl ChatModel for TriageHandle {
    async fn answer(
        &self,
        question: ChatQuestion,
        sink: Arc<dyn AnswerSink>,
    ) -> Result<ChatAnswer, TriageError> {
        self.chat_streaming(
            question.host,
            &question.overview,
            &question.history,
            &question.question,
            sink,
        )
        .await
    }
}

/// The model used when triage is not running: every question fails as unavailable.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisabledModel;

#[async_trait]
impl ChatModel for DisabledModel {
    async fn answer(
        &self,
        _question: ChatQuestion,
        _sink: Arc<dyn AnswerSink>,
    ) -> Result<ChatAnswer, TriageError> {
        Err(TriageError::Provider("triage is off".to_owned()))
    }
}
