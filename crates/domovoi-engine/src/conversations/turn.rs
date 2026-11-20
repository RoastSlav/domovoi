//! One background turn: asks the model for a pending answer and reports the result to the
//! writer. It is independent of every HTTP request and event stream; closing a panel never
//! stops it. Only the writer commits.

use std::sync::{Arc, PoisonError};

use domovoi_core::ConversationId;
use domovoi_core::conversation::{MAX_FAILURE_DETAIL_CHARS, Message, MessageFailure, MessageRole};
use domovoi_core::ports::Redactor;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::handle::Command;
use super::model::ChatQuestion;
use super::sink::TurnSink;
use super::writer::Registry;
use super::{ChatContext, ConversationDeps, Outcome, cut};
use crate::triage::{ChatRole, ChatTurn, TriageError};

/// How many earlier complete messages the model is shown.
const HISTORY_TURNS: u16 = 20;

/// Everything one turn needs.
pub(super) struct TurnJob {
    pub deps: Arc<ConversationDeps>,
    pub tx: mpsc::Sender<Command>,
    pub registry: Registry,
    pub token: CancellationToken,
    pub conversation: ConversationId,
    pub user: Message,
    pub assistant: Message,
    pub context: ChatContext,
}

/// Starts the turn and a supervisor that turns a panic into a stored failure.
pub(super) fn spawn(job: TurnJob) {
    let tx = job.tx.clone();
    let registry = job.registry.clone();
    let conversation = job.conversation;
    let turn = job.assistant.turn;
    let inner = tokio::spawn(run(job));
    tokio::spawn(async move {
        let finished = inner.await;
        registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&turn);
        if let Err(error) = finished
            && !error.is_cancelled()
        {
            let outcome = Outcome::Failed(
                MessageFailure::Internal,
                "the answer stopped unexpectedly".to_owned(),
            );
            let _ = tx.send(Command::Settle(conversation, turn, outcome)).await;
        }
    });
}

async fn run(job: TurnJob) {
    let TurnJob {
        deps,
        tx,
        token,
        conversation,
        user,
        assistant,
        context,
        ..
    } = job;
    let turn = assistant.turn;
    let history = match deps
        .conversations
        .recent_complete(conversation, user.ordinal, HISTORY_TURNS)
        .await
    {
        Ok(messages) => messages
            .into_iter()
            .map(|m| ChatTurn {
                role: match m.role {
                    MessageRole::User => ChatRole::User,
                    MessageRole::Assistant => ChatRole::Assistant,
                },
                text: m.text,
            })
            .collect(),
        Err(error) => {
            let outcome =
                Outcome::Failed(MessageFailure::Internal, detail(&error, &*deps.redactor));
            let _ = tx.send(Command::Settle(conversation, turn, outcome)).await;
            return;
        }
    };
    let sink = Arc::new(TurnSink::new(
        deps.live.clone(),
        conversation,
        turn,
        assistant.id,
        deps.redactor.clone(),
        deps.clock.clone(),
    ));
    let question = ChatQuestion {
        host: context.tool_host,
        overview: context.overview,
        history,
        question: user.text,
    };
    let result = tokio::select! {
        // Stopped by the person (already committed) or by shutdown (recovered at the next start).
        () = token.cancelled() => return,
        result = deps.model.answer(question, sink) => result,
    };
    let outcome = match result {
        Ok(answer) => Outcome::Answered(answer),
        Err(error) => {
            let code = match &error {
                TriageError::BudgetSpent => MessageFailure::BudgetSpent,
                TriageError::Busy { .. } => MessageFailure::Busy,
                TriageError::Provider(_) | TriageError::AdmissionUnavailable => {
                    MessageFailure::ModelUnavailable
                }
                TriageError::Store(_) | TriageError::UnknownIncident(_) => MessageFailure::Internal,
            };
            Outcome::Failed(code, detail(&error, &*deps.redactor))
        }
    };
    let _ = tx.send(Command::Settle(conversation, turn, outcome)).await;
}

/// The redacted, cut explanation stored with a failure.
fn detail(error: &dyn std::fmt::Display, redactor: &dyn Redactor) -> String {
    cut(
        &redactor.redact(&error.to_string()),
        MAX_FAILURE_DETAIL_CHARS,
    )
}
