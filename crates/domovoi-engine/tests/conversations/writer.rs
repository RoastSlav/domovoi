//! Commands of the conversation writer: create, send, replay, attach, delete.

use domovoi_core::conversation::{
    ConversationScope, MAX_MESSAGES, MessageFailure, MessageRole, MessageStatus,
};
use domovoi_core::ports::{AuditFilter, AuditLog, ConversationRepo, LiveEnd, LiveEvent, Page};
use domovoi_core::{HostId, OperationId, TurnId};
use domovoi_engine::conversations::{ConversationError, NewConversation};
use domovoi_engine::triage::{ChatAnswer, ToolUse};

use crate::harness::{Rig, SECRET, person, within};

fn tool(name: &str, failed: bool) -> ToolUse {
    ToolUse {
        tool: name.to_owned(),
        skill: None,
        failed,
        refused: false,
        raw: None,
    }
}

#[tokio::test]
async fn a_question_commits_the_user_message_and_a_pending_answer_together() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "Why is the NAS slow?").await;

    assert_eq!(started.user.status, MessageStatus::Complete);
    assert_eq!(started.assistant.status, MessageStatus::Pending);
    assert_eq!(started.user.turn, started.assistant.turn);
    assert!(!started.replayed);
    let stored = rig.messages(conversation.id).await;
    assert_eq!(stored.len(), 2);
    let row = rig.store.get(conversation.id).await.unwrap().unwrap().value;
    assert_eq!(row.last_ordinal, 2);
    assert_eq!(row.title, "Why is the NAS slow?");

    let rows = rig.journal_of("conversation.message").await;
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.payload.to_string().contains("NAS"));
    }
    rig.wait_event(conversation.id, |e| matches!(e, LiveEvent::Start { .. }))
        .await;
    rig.model.release();
}

#[tokio::test]
async fn the_answer_is_committed_with_its_tools_and_a_journal_row() {
    let rig = Rig::new().await;
    rig.model.push(Ok(ChatAnswer {
        text: "Two restarts.".to_owned(),
        tools: vec![tool("journal", false), tool("docker_ps", true)],
    }));
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "what happened?").await;

    let answer = rig
        .wait_assistant(
            conversation.id,
            started.assistant.turn,
            MessageStatus::Complete,
        )
        .await;
    assert_eq!(answer.text, "Two restarts.");
    assert_eq!(answer.tools.len(), 2);
    assert!(answer.tools[1].failed);
    assert!(answer.completed_at.is_some());
    let event = rig
        .wait_event(conversation.id, |e| matches!(e, LiveEvent::End { .. }))
        .await;
    assert_eq!(
        event,
        LiveEvent::End {
            turn: started.assistant.turn,
            outcome: LiveEnd::Complete
        }
    );
    let complete = rig
        .journal_of("conversation.message")
        .await
        .into_iter()
        .filter(|e| e.payload["status"] == "complete" && e.payload["role"] == "assistant")
        .count();
    assert_eq!(complete, 1);
}

#[tokio::test]
async fn a_replayed_turn_returns_the_same_messages_and_calls_no_model() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    let turn = TurnId::new();
    let first = rig
        .handle
        .send(rig.send(conversation.id, turn, "hello"))
        .await
        .unwrap();
    rig.wait_assistant(conversation.id, turn, MessageStatus::Complete)
        .await;
    let second = rig
        .handle
        .send(rig.send(conversation.id, turn, "hello"))
        .await
        .unwrap();

    assert!(second.replayed);
    assert_eq!(second.user.id, first.user.id);
    assert_eq!(second.assistant.id, first.assistant.id);
    assert_eq!(rig.model.calls(), 1);
    assert_eq!(rig.messages(conversation.id).await.len(), 2);
}

#[tokio::test]
async fn a_second_question_while_one_is_pending_is_refused() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    rig.ask(conversation.id, "one").await;
    let refused = rig
        .handle
        .send(rig.send(conversation.id, TurnId::new(), "two"))
        .await;
    assert_eq!(refused.unwrap_err(), ConversationError::TurnInProgress);
    assert_eq!(rig.messages(conversation.id).await.len(), 2);
    rig.model.release();
}

#[tokio::test]
async fn a_full_conversation_refuses_another_question() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    for i in 0..(MAX_MESSAGES / 2) {
        let started = rig.ask(conversation.id, &format!("q{i}")).await;
        rig.wait_assistant(
            conversation.id,
            started.assistant.turn,
            MessageStatus::Complete,
        )
        .await;
    }
    let refused = rig
        .handle
        .send(rig.send(conversation.id, TurnId::new(), "one more"))
        .await;
    assert_eq!(refused.unwrap_err(), ConversationError::Full);
}

#[tokio::test]
async fn bad_questions_are_refused_and_nothing_is_stored() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    for text in ["", "   \n", &"x".repeat(1001)] {
        let result = rig
            .handle
            .send(rig.send(conversation.id, TurnId::new(), text))
            .await;
        assert_eq!(result.unwrap_err(), ConversationError::BadQuestion);
    }
    assert!(rig.messages(conversation.id).await.is_empty());
}

#[tokio::test]
async fn the_question_is_redacted_before_it_is_stored() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    let started = rig
        .ask(conversation.id, &format!("my password is {SECRET} ok"))
        .await;
    assert!(!started.user.text.contains(SECRET));
    assert!(started.user.text.contains("[REDACTED]"));
    let row = rig.store.get(conversation.id).await.unwrap().unwrap().value;
    assert!(!row.title.contains(SECRET));
    rig.wait_assistant(
        conversation.id,
        started.assistant.turn,
        MessageStatus::Complete,
    )
    .await;
    let asked = rig.model.questions.lock().unwrap()[0].question.clone();
    assert!(!asked.contains(SECRET));
}

#[tokio::test]
async fn global_scope_is_explicit() {
    let rig = Rig::new().await;
    let global = rig.global().await;
    assert_eq!(global.scope, ConversationScope::Global);
    let host = HostId::new();
    let hosted = rig
        .handle
        .create(NewConversation {
            scope: ConversationScope::Host(host),
            operation: None,
            actor: person(),
        })
        .await
        .unwrap();
    let stored = rig.store.get(hosted.id).await.unwrap().unwrap().value;
    assert_eq!(stored.scope, ConversationScope::Host(host));
    assert_eq!(rig.journal_of("conversation.created").await.len(), 2);
}

#[tokio::test]
async fn set_operation_attaches_and_detaches() {
    let rig = Rig::new().await;
    let conversation = rig.global().await;
    let operation = OperationId::new();
    let attached = rig
        .handle
        .set_operation(conversation.id, Some(operation), person())
        .await
        .unwrap();
    assert_eq!(attached.operation, Some(operation));
    let detached = rig
        .handle
        .set_operation(conversation.id, None, person())
        .await
        .unwrap();
    assert_eq!(detached.operation, None);

    let rows = rig.journal_of("conversation.updated").await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].payload["change"], "operation_attached");
    assert_eq!(rows[1].payload["change"], "operation_detached");
    let audit = AuditLog::list(&*rig.store, &AuditFilter::default(), &Page::first(50))
        .await
        .unwrap();
    assert!(audit.items.is_empty());
}

#[tokio::test]
async fn delete_removes_the_conversation_and_audits_it() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    assert_eq!(
        rig.handle
            .delete(conversation.id, person())
            .await
            .unwrap_err(),
        ConversationError::TurnInProgress
    );
    rig.model.release();
    rig.wait_assistant(
        conversation.id,
        started.assistant.turn,
        MessageStatus::Complete,
    )
    .await;

    rig.handle.delete(conversation.id, person()).await.unwrap();
    assert!(rig.store.get(conversation.id).await.unwrap().is_none());
    assert!(rig.messages(conversation.id).await.is_empty());
    assert_eq!(rig.journal_of("conversation.deleted").await.len(), 1);
    let filter = AuditFilter {
        kind: Some("conversation.deleted".to_owned()),
        ..AuditFilter::default()
    };
    let audit = AuditLog::list(&*rig.store, &filter, &Page::first(10))
        .await
        .unwrap();
    assert_eq!(audit.items.len(), 1);
    assert_eq!(audit.items[0].detail["messages"], 2);
    assert_eq!(
        rig.handle
            .delete(conversation.id, person())
            .await
            .unwrap_err(),
        ConversationError::NotFound
    );
}

#[tokio::test]
async fn the_turn_task_survives_the_command_that_started_it() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let handle = rig.handle.clone();
    let command = rig.send(conversation.id, TurnId::new(), "q");
    let started = tokio::spawn(async move { handle.send(command).await })
        .await
        .unwrap()
        .unwrap();
    rig.model.release();
    rig.wait_assistant(
        conversation.id,
        started.assistant.turn,
        MessageStatus::Complete,
    )
    .await;
}

#[tokio::test]
async fn closing_panel_does_not_stop_operation() {
    let rig = Rig::new().await;
    rig.model.hold();
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    // Every handle and every watcher goes away; the turn must still finish and commit.
    let crate::harness::Rig {
        store,
        model,
        handle,
        ..
    } = rig;
    drop(handle);
    model.release();
    within(async {
        loop {
            let done = store
                .messages(conversation.id)
                .await
                .unwrap()
                .iter()
                .any(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Complete);
            if done {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_ne!(started.assistant.failure, Some(MessageFailure::Cancelled));
}
