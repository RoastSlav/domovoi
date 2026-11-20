//! The one writer of the conversation aggregate: loads fresh state, decides in the pure
//! builders of `commit`, commits one transaction, answers the caller.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::actor::Actor;
use domovoi_core::conversation::{
    Conversation, MAX_MESSAGES, MAX_QUESTION_CHARS, Message, MessageFailure, MessageRole,
    MessageStatus,
};
use domovoi_core::ports::{CommitError, LiveEnd, LiveEvent, StoreError, Transaction};
use domovoi_core::{ConversationId, OperationId, TurnId};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::commit::{self, Finish};
use super::handle::{Command, ConversationHandle};
use super::turn::{self, TurnJob};
use super::{
    ChatContext, ConversationDeps, ConversationError, NewConversation, Outcome, RetryTurn,
    SendMessage, TurnStarted, recover,
};

/// The running turns and the tokens that stop them.
pub(super) type Registry = Arc<Mutex<HashMap<TurnId, CancellationToken>>>;

/// How many commands may wait for the writer.
const QUEUE: usize = 64;

/// Why one attempt at a command did not finish.
enum Step {
    /// A guarded write found something else than what was read.
    Conflict,
    /// The unique pending index (or another constraint) refused the write.
    Constraint,
    /// A definite failure.
    Fail(ConversationError),
}

impl Step {
    fn into_error(self, constraint: ConversationError) -> ConversationError {
        match self {
            Self::Conflict => {
                ConversationError::Store("the conversation kept changing; try again".to_owned())
            }
            Self::Constraint => constraint,
            Self::Fail(error) => error,
        }
    }
}

fn store(error: &StoreError) -> Step {
    Step::Fail(ConversationError::Store(error.to_string()))
}

mod settle;

struct Writer {
    deps: Arc<ConversationDeps>,
    tx: mpsc::WeakSender<Command>,
    registry: Registry,
}

pub(super) async fn spawn(
    deps: ConversationDeps,
) -> Result<(ConversationHandle, JoinHandle<()>), ConversationError> {
    let deps = Arc::new(deps);
    recover::run(&deps).await?;
    let (tx, mut rx) = mpsc::channel(QUEUE);
    let writer = Writer {
        deps: deps.clone(),
        tx: tx.downgrade(),
        registry: Registry::default(),
    };
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = deps.shutdown.cancelled() => break,
                command = rx.recv() => match command {
                    Some(command) => writer.handle(command).await,
                    None => break,
                },
            }
        }
    });
    Ok((ConversationHandle::new(tx), join))
}

impl Writer {
    async fn handle(&self, command: Command) {
        match command {
            Command::Create(new, reply) => {
                let _ = reply.send(self.create(new).await);
            }
            Command::Send(cmd, reply) => {
                let _ = reply.send(self.send(cmd).await);
            }
            Command::Retry(cmd, reply) => {
                let _ = reply.send(self.retry(cmd).await);
            }
            Command::Cancel(conversation, turn, actor, reply) => {
                let _ = reply.send(self.cancel(conversation, turn, &actor).await);
            }
            Command::SetOperation(conversation, operation, actor, reply) => {
                let _ = reply.send(self.set_operation(conversation, operation, actor).await);
            }
            Command::Delete(conversation, actor, reply) => {
                let _ = reply.send(self.delete(conversation, actor).await);
            }
            Command::Settle(conversation, turn, outcome) => {
                self.settle(conversation, turn, outcome).await;
            }
        }
    }

    async fn commit(&self, tx: Transaction) -> Result<(), Step> {
        match self.deps.uow.commit(tx).await {
            Ok(_) => Ok(()),
            Err(CommitError::Conflict { .. }) => Err(Step::Conflict),
            Err(CommitError::Constraint(_)) => Err(Step::Constraint),
            Err(other) => Err(Step::Fail(ConversationError::Store(other.to_string()))),
        }
    }

    async fn load(&self, id: ConversationId) -> Result<(Conversation, u64), Step> {
        match self.deps.conversations.get(id).await {
            Ok(Some(found)) => Ok((found.value, found.version)),
            Ok(None) => Err(Step::Fail(ConversationError::NotFound)),
            Err(error) => Err(store(&error)),
        }
    }

    async fn messages(&self, id: ConversationId) -> Result<Vec<Message>, Step> {
        self.deps
            .conversations
            .messages(id)
            .await
            .map_err(|error| store(&error))
    }

    /// Runs `attempt` again once after a conflict, as every writer does.
    async fn retried<T, F, Fut>(
        &self,
        constraint: ConversationError,
        attempt: F,
    ) -> Result<T, ConversationError>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, Step>>,
    {
        for _ in 0..2 {
            match attempt().await {
                Err(Step::Conflict) => {}
                other => return other.map_err(|step| step.into_error(constraint.clone())),
            }
        }
        Err(Step::Conflict.into_error(constraint))
    }

    async fn create(&self, new: NewConversation) -> Result<Conversation, ConversationError> {
        let now = self.deps.clock.now();
        let conversation = Conversation {
            id: ConversationId::new(),
            title: String::new(),
            scope: new.scope,
            operation: new.operation,
            last_ordinal: 0,
            created_at: now,
            updated_at: now,
        };
        self.commit(commit::create(&conversation, new.actor))
            .await
            .map_err(|step| step.into_error(ConversationError::TurnInProgress))?;
        Ok(conversation)
    }

    async fn send(&self, cmd: SendMessage) -> Result<TurnStarted, ConversationError> {
        let text = cmd.text.trim();
        let length = text.chars().count();
        if length == 0 || length > MAX_QUESTION_CHARS {
            return Err(ConversationError::BadQuestion);
        }
        let redacted = self.deps.redactor.redact(text);
        let started = self
            .retried(ConversationError::TurnInProgress, || {
                self.send_once(&cmd, &redacted)
            })
            .await?;
        if !started.replayed {
            self.start_turn(&started, cmd.conversation, cmd.context);
        }
        Ok(started)
    }

    async fn send_once(&self, cmd: &SendMessage, redacted: &str) -> Result<TurnStarted, Step> {
        let (conversation, version) = self.load(cmd.conversation).await?;
        let messages = self.messages(cmd.conversation).await?;
        if let Some(user) = messages
            .iter()
            .find(|m| m.role == MessageRole::User && m.turn == cmd.turn)
        {
            let assistant = messages
                .iter()
                .find(|m| m.role == MessageRole::Assistant && m.ordinal == user.ordinal + 1)
                .ok_or_else(|| {
                    Step::Fail(ConversationError::Store(
                        "a question has no answer row".to_owned(),
                    ))
                })?;
            return Ok(TurnStarted {
                user: user.clone(),
                assistant: assistant.clone(),
                replayed: true,
            });
        }
        if messages.iter().any(|m| m.status == MessageStatus::Pending) {
            return Err(Step::Fail(ConversationError::TurnInProgress));
        }
        if messages.len() + 2 > MAX_MESSAGES {
            return Err(Step::Fail(ConversationError::Full));
        }
        let now = self.deps.clock.now();
        let sent = commit::send(
            &conversation,
            version,
            redacted,
            cmd.turn,
            now,
            cmd.actor.clone(),
        );
        self.commit(sent.tx).await?;
        Ok(TurnStarted {
            user: sent.user,
            assistant: sent.assistant,
            replayed: false,
        })
    }

    async fn retry(&self, cmd: RetryTurn) -> Result<TurnStarted, ConversationError> {
        let started = self
            .retried(ConversationError::TurnInProgress, || self.retry_once(&cmd))
            .await?;
        self.start_turn(&started, cmd.conversation, cmd.context);
        Ok(started)
    }

    async fn retry_once(&self, cmd: &RetryTurn) -> Result<TurnStarted, Step> {
        let (conversation, version) = self.load(cmd.conversation).await?;
        let messages = self.messages(cmd.conversation).await?;
        let target = messages
            .iter()
            .find(|m| m.id == cmd.message)
            .ok_or(Step::Fail(ConversationError::NotFound))?;
        if messages.iter().any(|m| m.status == MessageStatus::Pending) {
            return Err(Step::Fail(ConversationError::TurnInProgress));
        }
        let last = messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant);
        let retryable = matches!(
            target.status,
            MessageStatus::Failed | MessageStatus::Cancelled
        );
        if target.role != MessageRole::Assistant
            || last.map(|m| m.id) != Some(target.id)
            || !retryable
        {
            return Err(Step::Fail(ConversationError::NotRetryable));
        }
        let user = messages
            .iter()
            .find(|m| m.role == MessageRole::User && m.ordinal + 1 == target.ordinal)
            .ok_or(Step::Fail(ConversationError::NotRetryable))?;
        let now = self.deps.clock.now();
        let (tx, assistant) = commit::retry(&conversation, version, target, TurnId::new(), now);
        self.commit(tx).await?;
        Ok(TurnStarted {
            user: user.clone(),
            assistant,
            replayed: false,
        })
    }

    async fn cancel(
        &self,
        conversation: ConversationId,
        turn: TurnId,
        actor: &Actor,
    ) -> Result<MessageStatus, ConversationError> {
        let (status, stopped) = self
            .retried(ConversationError::TurnInProgress, || {
                self.cancel_once(conversation, turn, actor)
            })
            .await?;
        if stopped {
            if let Some(token) = self.take_token(turn) {
                token.cancel();
            }
            self.deps.live.publish(
                conversation,
                LiveEvent::End {
                    turn,
                    outcome: LiveEnd::Cancelled,
                },
            );
        }
        Ok(status)
    }

    async fn cancel_once(
        &self,
        conversation_id: ConversationId,
        turn: TurnId,
        _actor: &Actor,
    ) -> Result<(MessageStatus, bool), Step> {
        let (conversation, version) = self.load(conversation_id).await?;
        let messages = self.messages(conversation_id).await?;
        let assistant = messages
            .iter()
            .find(|m| m.role == MessageRole::Assistant && m.turn == turn)
            .ok_or(Step::Fail(ConversationError::NotFound))?;
        if assistant.status != MessageStatus::Pending {
            return Ok((assistant.status, false));
        }
        let end = Finish {
            status: MessageStatus::Cancelled,
            text: String::new(),
            tools: Vec::new(),
            failure: Some(MessageFailure::Cancelled),
            detail: None,
        };
        let (tx, _) = commit::finish(
            &conversation,
            version,
            assistant,
            end,
            self.deps.clock.now(),
        );
        self.commit(tx).await?;
        Ok((MessageStatus::Cancelled, true))
    }

    async fn set_operation(
        &self,
        conversation: ConversationId,
        operation: Option<OperationId>,
        actor: Actor,
    ) -> Result<Conversation, ConversationError> {
        self.retried(ConversationError::TurnInProgress, || async {
            let (current, version) = self.load(conversation).await?;
            let (tx, next) = commit::set_operation(
                &current,
                version,
                operation,
                self.deps.clock.now(),
                actor.clone(),
            );
            self.commit(tx).await?;
            Ok(next)
        })
        .await
    }

    async fn delete(
        &self,
        conversation: ConversationId,
        actor: Actor,
    ) -> Result<(), ConversationError> {
        self.retried(ConversationError::TurnInProgress, || async {
            let (_, version) = self.load(conversation).await?;
            let messages = self.messages(conversation).await?;
            if messages.iter().any(|m| m.status == MessageStatus::Pending) {
                return Err(Step::Fail(ConversationError::TurnInProgress));
            }
            let tx = commit::delete(
                conversation,
                version,
                messages.len(),
                self.deps.clock.now(),
                actor.clone(),
            );
            self.commit(tx).await
        })
        .await
    }

    fn take_token(&self, turn: TurnId) -> Option<CancellationToken> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&turn)
            .cloned()
    }

    /// Starts the background turn for a committed, pending answer.
    fn start_turn(
        &self,
        started: &TurnStarted,
        conversation: ConversationId,
        context: ChatContext,
    ) {
        let Some(tx) = self.tx.upgrade() else {
            return;
        };
        let token = self.deps.shutdown.child_token();
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(started.assistant.turn, token.clone());
        turn::spawn(TurnJob {
            deps: self.deps.clone(),
            tx,
            registry: self.registry.clone(),
            token,
            conversation,
            user: started.user.clone(),
            assistant: started.assistant.clone(),
            context,
        });
    }
}
