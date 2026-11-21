//! A rig around the real conversation service with a scripted model.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use domovoi_core::actor::Actor;
use domovoi_core::conversation::{
    Conversation, ConversationScope, Message, MessageRole, MessageStatus,
};
use domovoi_core::ports::{
    AnswerSink, ConversationRepo, JournalEntry, JournalRead, LiveEvent, UnitOfWork,
};
use domovoi_core::{ConversationId, TurnId, UserId};
use domovoi_engine::conversations::{
    ChatContext, ChatModel, ChatQuestion, ConversationDeps, ConversationHandle,
    ConversationService, NewConversation, SendMessage, TurnStarted,
};
use domovoi_engine::triage::{ChatAnswer, TriageError};
use domovoi_testkit::{FakeClock, InMemoryStore, ListRedactor, RecordingLive};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The secret the rig's redactor knows.
pub const SECRET: &str = "hunter2hunter2";

/// Sets a flag when dropped, so a test can see that a model call was cancelled.
struct DropFlag {
    flag: Arc<AtomicBool>,
    done: bool,
}

impl Drop for DropFlag {
    fn drop(&mut self) {
        if !self.done {
            self.flag.store(true, Ordering::SeqCst);
        }
    }
}

/// A scripted [`ChatModel`] that can be held until a test releases it.
pub struct FakeChatModel {
    script: Mutex<VecDeque<Result<ChatAnswer, TriageError>>>,
    fragments: Mutex<Vec<String>>,
    held: AtomicBool,
    gate: Notify,
    entered: Notify,
    calls: AtomicUsize,
    /// Every question the model was asked.
    pub questions: Mutex<Vec<ChatQuestion>>,
    /// Set when a model call was dropped before it finished.
    pub dropped: Arc<AtomicBool>,
}

impl FakeChatModel {
    fn new() -> Self {
        Self {
            script: Mutex::new(VecDeque::new()),
            fragments: Mutex::new(Vec::new()),
            held: AtomicBool::new(false),
            gate: Notify::new(),
            entered: Notify::new(),
            calls: AtomicUsize::new(0),
            questions: Mutex::new(Vec::new()),
            dropped: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Queues the next result.
    pub fn push(&self, result: Result<ChatAnswer, TriageError>) {
        self.script.lock().unwrap().push_back(result);
    }

    /// Queues an answer with `text` and no tools.
    pub fn push_text(&self, text: &str) {
        self.push(Ok(ChatAnswer {
            text: text.to_owned(),
            tools: Vec::new(),
        }));
    }

    /// The next call streams these fragments to the sink before it answers.
    pub fn stream(&self, fragments: &[&str]) {
        *self.fragments.lock().unwrap() = fragments.iter().map(|f| (*f).to_owned()).collect();
    }

    /// Makes every call wait until [`release`](Self::release).
    pub fn hold(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Lets one held call finish.
    pub fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
        self.gate.notify_one();
    }

    /// Waits until a call has started.
    pub async fn entered(&self) {
        within(self.entered.notified()).await;
    }

    /// How many times the model was asked.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChatModel for FakeChatModel {
    async fn answer(
        &self,
        question: ChatQuestion,
        sink: Arc<dyn AnswerSink>,
    ) -> Result<ChatAnswer, TriageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.questions.lock().unwrap().push(question);
        let mut guard = DropFlag {
            flag: self.dropped.clone(),
            done: false,
        };
        sink.attempt_started();
        let fragments = std::mem::take(&mut *self.fragments.lock().unwrap());
        for fragment in &fragments {
            sink.text(fragment);
        }
        self.entered.notify_one();
        if self.held.load(Ordering::SeqCst) {
            self.gate.notified().await;
        }
        guard.done = true;
        self.script.lock().unwrap().pop_front().unwrap_or_else(|| {
            Ok(ChatAnswer {
                text: "ok".to_owned(),
                tools: Vec::new(),
            })
        })
    }
}

/// Bounds a wait so a broken test fails instead of hanging.
pub async fn within<T>(future: impl Future<Output = T>) -> T {
    match tokio::time::timeout(Duration::from_secs(10), future).await {
        Ok(value) => value,
        Err(_) => panic!("timed out waiting"),
    }
}

/// The service under test with its fakes.
pub struct Rig {
    pub store: Arc<InMemoryStore>,
    pub live: Arc<RecordingLive>,
    pub model: Arc<FakeChatModel>,
    pub handle: ConversationHandle,
    pub shutdown: CancellationToken,
    pub join: JoinHandle<()>,
}

/// The person acting in tests.
pub fn person() -> Actor {
    Actor::User(UserId::new())
}

/// A context with no tools.
pub fn context() -> ChatContext {
    ChatContext {
        overview: "overview".to_owned(),
        tool_host: None,
    }
}

impl Rig {
    /// A rig over an empty store.
    pub async fn new() -> Self {
        Self::over(Arc::new(InMemoryStore::new())).await
    }

    /// A rig over an existing store (a hub restart).
    pub async fn over(store: Arc<InMemoryStore>) -> Self {
        let clock = Arc::new(FakeClock::new("2026-11-18T10:00:00Z".parse().unwrap()));
        let live = Arc::new(RecordingLive::new());
        let model = Arc::new(FakeChatModel::new());
        let shutdown = CancellationToken::new();
        let (handle, join) = ConversationService::spawn(ConversationDeps {
            conversations: store.clone() as Arc<dyn ConversationRepo>,
            uow: store.clone() as Arc<dyn UnitOfWork>,
            model: model.clone(),
            live: live.clone(),
            redactor: Arc::new(ListRedactor::new([SECRET])),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
        })
        .await
        .unwrap();
        Self {
            store,
            live,
            model,
            handle,
            shutdown,
            join,
        }
    }

    /// A global conversation.
    pub async fn global(&self) -> Conversation {
        self.handle
            .create(NewConversation {
                scope: ConversationScope::Global,
                operation: None,
                actor: person(),
            })
            .await
            .unwrap()
    }

    /// Sends `text` under a fresh turn.
    pub async fn ask(&self, conversation: ConversationId, text: &str) -> TurnStarted {
        self.handle
            .send(self.send(conversation, TurnId::new(), text))
            .await
            .unwrap()
    }

    /// A send command.
    pub fn send(&self, conversation: ConversationId, turn: TurnId, text: &str) -> SendMessage {
        SendMessage {
            conversation,
            turn,
            text: text.to_owned(),
            context: context(),
            actor: person(),
        }
    }

    /// The stored messages of a conversation.
    pub async fn messages(&self, conversation: ConversationId) -> Vec<Message> {
        self.store.messages(conversation).await.unwrap()
    }

    /// Waits until the assistant message answering `turn` has `status`.
    pub async fn wait_assistant(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        status: MessageStatus,
    ) -> Message {
        within(async {
            loop {
                let found = self
                    .messages(conversation)
                    .await
                    .into_iter()
                    .find(|m| m.role == MessageRole::Assistant && m.turn == turn);
                if let Some(message) = found.filter(|m| m.status == status) {
                    return message;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    /// Waits until `conversation` has seen an event matching `matches`.
    pub async fn wait_event(
        &self,
        conversation: ConversationId,
        matches: impl Fn(&LiveEvent) -> bool,
    ) -> LiveEvent {
        within(async {
            loop {
                if let Some(event) = self.live.events_of(conversation).into_iter().find(&matches) {
                    return event;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    /// Every journal row, oldest first.
    pub async fn journal(&self) -> Vec<JournalEntry> {
        self.store.entries_after(0, 500).await.unwrap()
    }

    /// Journal rows of one kind.
    pub async fn journal_of(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal()
            .await
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }
}
