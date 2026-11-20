//! The conversation service crashed at every commit of a short exchange: each command is one
//! atomic commit, so a crash leaves the question and its pending answer together or neither,
//! and after a restart no answer is left pending.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use domovoi_core::conversation::{ConversationScope, MessageStatus};
use domovoi_core::ports::{AnswerSink, ConversationRepo, UnitOfWork};
use domovoi_core::{Actor, TurnId, UserId};
use domovoi_engine::conversations::{
    ChatContext, ChatModel, ChatQuestion, ConversationDeps, ConversationService, NewConversation,
    SendMessage,
};
use domovoi_engine::triage::{ChatAnswer, TriageError};
use domovoi_testkit::build::T0;
use domovoi_testkit::crash::{CrashPlan, CrashPoint, CrashingStore};
use domovoi_testkit::{FakeClock, InMemoryStore, ListRedactor, RecordingLive};
use tokio_util::sync::CancellationToken;

/// A model that answers at once.
struct Instant;

#[async_trait]
impl ChatModel for Instant {
    async fn answer(
        &self,
        _question: ChatQuestion,
        _sink: Arc<dyn AnswerSink>,
    ) -> Result<ChatAnswer, TriageError> {
        Ok(ChatAnswer {
            text: "done".to_owned(),
            tools: Vec::new(),
        })
    }
}

fn deps(
    store: &Arc<InMemoryStore>,
    uow: Arc<dyn UnitOfWork>,
    shutdown: &CancellationToken,
) -> ConversationDeps {
    ConversationDeps {
        conversations: store.clone() as Arc<dyn ConversationRepo>,
        uow,
        model: Arc::new(Instant),
        live: Arc::new(RecordingLive::new()),
        redactor: Arc::new(ListRedactor::new(["hunter2hunter2"])),
        clock: Arc::new(FakeClock::new(T0)),
        shutdown: shutdown.clone(),
    }
}

#[tokio::test]
async fn conversations_survive_a_crash_at_every_commit_in_memory() {
    for at_commit in 1..=3 {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let store = Arc::new(InMemoryStore::new());
            let crashing = Arc::new(CrashingStore::new(store.clone()));
            let shutdown = CancellationToken::new();
            let (handle, join) =
                ConversationService::spawn(deps(&store, crashing.clone(), &shutdown))
                    .await
                    .unwrap();
            crashing.arm(CrashPlan { at_commit, point });
            let actor = Actor::User(UserId::new());
            let created = handle
                .create(NewConversation {
                    scope: ConversationScope::Global,
                    operation: None,
                    actor: actor.clone(),
                })
                .await;
            if let Ok(conversation) = &created {
                let _ = handle
                    .send(SendMessage {
                        conversation: conversation.id,
                        turn: TurnId::new(),
                        text: "why".to_owned(),
                        context: ChatContext {
                            overview: String::new(),
                            tool_host: None,
                        },
                        actor,
                    })
                    .await;
            }
            // The settle commit runs in the turn task: wait until the crash fires.
            for _ in 0..2000 {
                if crashing.crashed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            assert!(
                crashing.crashed(),
                "commit {at_commit}, {point:?}: the crash never happened"
            );
            shutdown.cancel();
            let _ = join.await;

            // A restart over the same store.
            crashing.restart();
            let shutdown = CancellationToken::new();
            let (_handle, join) = ConversationService::spawn(deps(
                &store,
                store.clone() as Arc<dyn UnitOfWork>,
                &shutdown,
            ))
            .await
            .unwrap();
            assert!(
                store.pending().await.unwrap().is_empty(),
                "commit {at_commit}, {point:?}: an answer was left pending after restart"
            );
            let rows = store
                .list(&Default::default(), &domovoi_core::ports::Page::first(50))
                .await
                .unwrap();
            for row in rows.items {
                let messages = store.messages(row.conversation.id).await.unwrap();
                assert!(
                    messages.is_empty() || messages.len() == 2,
                    "commit {at_commit}, {point:?}: a question without its answer row"
                );
                assert!(messages.iter().all(|m| m.status != MessageStatus::Pending));
            }
            shutdown.cancel();
            let _ = join.await;
        }
    }
}
