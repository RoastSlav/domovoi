//! Turn semantics: failures, retry, cancel, stale results and the model's history.

use std::sync::atomic::Ordering;

use domovoi_core::conversation::{MessageFailure, MessageRole, MessageStatus};
use domovoi_core::ports::{
    ConversationWrite, LiveEnd, LiveEvent, MessageGuard, StateWrite, Transaction, UnitOfWork,
};
use domovoi_engine::conversations::{ConversationError, RetryTurn};
use domovoi_engine::triage::{ChatRole, TriageError};

use crate::harness::{Rig, SECRET, context, person, within};

#[tokio::test]
async fn cancel_marks_the_turn_stopped_and_cancels_the_model_call() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    rig.model.entered().await;

    let status = rig
        .handle
        .cancel(conversation.id, started.assistant.turn, person())
        .await
        .unwrap();
    assert_eq!(status, MessageStatus::Cancelled);
    let stored = rig.messages(conversation.id).await.pop().unwrap();
    assert_eq!(stored.status, MessageStatus::Cancelled);
    assert_eq!(stored.failure, Some(MessageFailure::Cancelled));
    assert!(stored.text.is_empty());
    within(async {
        while !rig.model.dropped.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    rig.wait_event(conversation.id, |e| {
        matches!(
            e,
            LiveEvent::End {
                outcome: LiveEnd::Cancelled,
                ..
            }
        )
    })
    .await;
}

#[tokio::test]
async fn stale_reply_discarded() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    rig.model.entered().await;
    rig.model.push_text("too late");

    // The pending message is settled by something else while the model is still working.
    let mut stopped = rig.messages(conversation.id).await.pop().unwrap();
    stopped.status = MessageStatus::Cancelled;
    stopped.failure = Some(MessageFailure::Cancelled);
    let tx = Transaction::new().write(StateWrite::Conversation(ConversationWrite::Message {
        message: stopped,
        guard: MessageGuard::Status(MessageStatus::Pending),
    }));
    rig.store.commit(tx).await.unwrap();
    let rows_before = rig.journal().await.len();

    rig.model.release();
    let event = rig
        .wait_event(conversation.id, |e| matches!(e, LiveEvent::End { .. }))
        .await;
    assert_eq!(
        event,
        LiveEvent::End {
            turn: started.assistant.turn,
            outcome: LiveEnd::Discarded
        }
    );
    let stored = rig.messages(conversation.id).await.pop().unwrap();
    assert_eq!(stored.status, MessageStatus::Cancelled);
    assert!(stored.text.is_empty());
    assert_eq!(rig.journal().await.len(), rows_before);
}

#[tokio::test]
async fn cancel_of_a_settled_turn_changes_nothing() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    rig.wait_assistant(
        conversation.id,
        started.assistant.turn,
        MessageStatus::Complete,
    )
    .await;
    let rows = rig.journal().await.len();
    let status = rig
        .handle
        .cancel(conversation.id, started.assistant.turn, person())
        .await
        .unwrap();
    assert_eq!(status, MessageStatus::Complete);
    assert_eq!(rig.journal().await.len(), rows);
}

#[tokio::test]
async fn retry_reruns_the_last_failed_turn_with_a_new_turn_id() {
    let rig = Rig::new().await;
    rig.model.push(Err(TriageError::BudgetSpent));
    let conversation = rig.global().await;
    let first = rig.ask(conversation.id, "same question").await;
    let failed = rig
        .wait_assistant(conversation.id, first.assistant.turn, MessageStatus::Failed)
        .await;
    assert_eq!(failed.failure, Some(MessageFailure::BudgetSpent));

    rig.model.push_text("now it works");
    let again = rig
        .handle
        .retry(RetryTurn {
            conversation: conversation.id,
            message: failed.id,
            context: context(),
            actor: person(),
        })
        .await
        .unwrap();
    assert_eq!(again.assistant.id, failed.id);
    assert_eq!(again.assistant.status, MessageStatus::Pending);
    assert_ne!(again.assistant.turn, first.assistant.turn);
    assert!(again.assistant.failure.is_none());
    let done = rig
        .wait_assistant(
            conversation.id,
            again.assistant.turn,
            MessageStatus::Complete,
        )
        .await;
    assert_eq!(done.text, "now it works");

    let asked = rig.model.questions.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[0].question, asked[1].question);
    assert_eq!(asked[0].history, asked[1].history);

    let replay = rig
        .handle
        .send(rig.send(conversation.id, first.user.turn, "same question"))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(rig.model.calls(), 2);
}

#[tokio::test]
async fn retry_is_refused_unless_the_last_answer_failed_or_was_stopped() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    rig.model.push(Err(TriageError::BudgetSpent));
    let first = rig.ask(conversation.id, "one").await;
    let first_failed = rig
        .wait_assistant(conversation.id, first.assistant.turn, MessageStatus::Failed)
        .await;
    let second = rig.ask(conversation.id, "two").await;
    let second_done = rig
        .wait_assistant(
            conversation.id,
            second.assistant.turn,
            MessageStatus::Complete,
        )
        .await;
    let retry = |message| RetryTurn {
        conversation: conversation.id,
        message,
        context: context(),
        actor: person(),
    };
    assert_eq!(
        rig.handle.retry(retry(second_done.id)).await.unwrap_err(),
        ConversationError::NotRetryable
    );
    assert_eq!(
        rig.handle.retry(retry(first_failed.id)).await.unwrap_err(),
        ConversationError::NotRetryable
    );

    rig.model.hold();
    let third = rig.ask(conversation.id, "three").await;
    assert_eq!(
        rig.handle
            .retry(retry(third.assistant.id))
            .await
            .unwrap_err(),
        ConversationError::TurnInProgress
    );
    rig.model.release();
}

#[tokio::test]
async fn model_errors_are_stored_as_failure_codes() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    let long = format!("down {SECRET} {}", "x".repeat(600));
    let cases = [
        (TriageError::BudgetSpent, MessageFailure::BudgetSpent),
        (TriageError::Busy { retry_at: None }, MessageFailure::Busy),
        (
            TriageError::Provider(long),
            MessageFailure::ModelUnavailable,
        ),
        (
            TriageError::AdmissionUnavailable,
            MessageFailure::ModelUnavailable,
        ),
        (
            TriageError::Store("db".to_owned()),
            MessageFailure::Internal,
        ),
    ];
    for (error, code) in cases {
        rig.model.push(Err(error));
        let started = rig.ask(conversation.id, "q").await;
        let failed = rig
            .wait_assistant(
                conversation.id,
                started.assistant.turn,
                MessageStatus::Failed,
            )
            .await;
        assert_eq!(failed.failure, Some(code));
        let detail = failed.failure_detail.unwrap();
        assert!(!detail.contains(SECRET));
        assert!(detail.chars().count() <= 300);
        assert!(failed.text.is_empty());
        rig.wait_event(conversation.id, |e| {
            matches!(e, LiveEvent::End { turn, outcome: LiveEnd::Failed } if *turn == started.assistant.turn)
        })
        .await;
    }
}

#[tokio::test]
async fn history_sent_to_the_model_is_the_stored_complete_messages() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    for i in 0..15 {
        rig.model.push_text(&format!("a{i}"));
        let started = rig.ask(conversation.id, &format!("q{i}")).await;
        rig.wait_assistant(
            conversation.id,
            started.assistant.turn,
            MessageStatus::Complete,
        )
        .await;
    }
    rig.model.push(Err(TriageError::BudgetSpent));
    let failed = rig.ask(conversation.id, "failed one").await;
    rig.wait_assistant(
        conversation.id,
        failed.assistant.turn,
        MessageStatus::Failed,
    )
    .await;
    let last = rig.ask(conversation.id, "final").await;
    rig.wait_assistant(
        conversation.id,
        last.assistant.turn,
        MessageStatus::Complete,
    )
    .await;

    let asked = rig.model.questions.lock().unwrap().clone();
    let history = &asked.last().unwrap().history;
    assert_eq!(history.len(), 20);
    let texts: Vec<_> = history.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts.last(), Some(&"failed one"));
    assert!(!texts.contains(&"final"));
    assert_eq!(history[0].role, ChatRole::Assistant);
    let stored = rig.messages(conversation.id).await;
    let complete = stored
        .iter()
        .filter(|m| m.status == MessageStatus::Complete && m.role == MessageRole::Assistant)
        .count();
    assert_eq!(complete, 16);
}
