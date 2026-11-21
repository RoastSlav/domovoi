//! A hub restart never re-asks the model.

use domovoi_core::conversation::{MessageFailure, MessageStatus};

use crate::harness::Rig;

#[tokio::test]
async fn restart_marks_pending_turns_interrupted() {
    let first = Rig::new().await;
    first.model.hold();
    let conversation = first.global().await;
    let started = first.ask(conversation.id, "q").await;
    first.model.entered().await;
    let store = first.store.clone();
    first.shutdown.cancel();
    first.join.await.unwrap();

    let second = Rig::over(store).await;
    let message = second
        .messages(conversation.id)
        .await
        .into_iter()
        .find(|m| m.turn == started.assistant.turn && m.id == started.assistant.id)
        .unwrap();
    assert_eq!(message.status, MessageStatus::Failed);
    assert_eq!(message.failure, Some(MessageFailure::Interrupted));
    assert_eq!(second.model.calls(), 0);
    let rows = second.journal_of("conversation.message").await;
    let interrupted = rows
        .iter()
        .filter(|e| e.payload["failure"] == "interrupted")
        .count();
    assert_eq!(interrupted, 1);
    assert_eq!(rows.last().unwrap().actor, "system:operator");
}
