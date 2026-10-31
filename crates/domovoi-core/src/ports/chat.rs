//! Chat gateways and one-way notifiers.

use std::sync::Arc;

use async_trait::async_trait;
use jiff::SignedDuration;

use crate::card::Card;
use crate::interaction::{Interaction, InteractionReply};
use crate::severity::Severity;

/// A posted chat message, opaque outside its gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageRef {
    /// The platform that issued it.
    pub platform: String,
    /// The platform's message id.
    pub id: String,
}

/// A chat thread, opaque outside its gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThreadRef {
    /// The platform that issued it.
    pub platform: String,
    /// The platform's thread id.
    pub id: String,
}

/// Why a chat call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    /// Network or protocol failure; retrying may help.
    #[error("transport error: {0}")]
    Transport(String),
    /// The platform asked to slow down.
    #[error("rate limited")]
    RateLimited {
        /// How long to wait, when the platform said.
        retry_after: Option<SignedDuration>,
    },
    /// The message or thread no longer exists.
    #[error("not found")]
    NotFound,
    /// The platform refused the call (permissions, size limits, bad config).
    #[error("rejected: {0}")]
    Rejected(String),
}

/// Receives interactions from a gateway. Implemented by the hub's interaction authorizer,
/// which alone decides whether the platform user may do anything.
#[async_trait]
pub trait InteractionSink: Send + Sync {
    /// Handles one interaction and says what to tell the user.
    async fn deliver(&self, i: Interaction) -> InteractionReply;
}

/// A two-way chat platform: cards with buttons, threads, and incoming interactions.
///
/// The gateway only translates. It never authorizes; every interaction goes to the sink.
#[async_trait]
pub trait ChatGateway: Send + Sync {
    /// Platform name, for example `discord`.
    fn platform(&self) -> &str;

    /// Posts a new card in the configured channel.
    async fn post_card(&self, card: &Card) -> Result<MessageRef, ChatError>;

    /// Replaces a posted card in place. [`ChatError::NotFound`] if it was deleted.
    async fn update_card(&self, msg: &MessageRef, card: &Card) -> Result<(), ChatError>;

    /// Opens a thread on a posted message. Opening a second thread on the same message
    /// returns the existing one.
    async fn open_thread(&self, msg: &MessageRef, title: &str) -> Result<ThreadRef, ChatError>;

    /// Posts plain text in a thread.
    async fn reply(&self, thread: &ThreadRef, text: &str) -> Result<(), ChatError>;

    /// Receives platform events and delivers each as an [`Interaction`] to `sink`, answering
    /// the user with the reply. Runs until the gateway shuts down or fails for good.
    async fn run(&self, sink: Arc<dyn InteractionSink>) -> Result<(), ChatError>;
}

/// A one-way message such as a digest or an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    /// Short title.
    pub title: String,
    /// Body in Markdown.
    pub markdown: String,
    /// Severity, for notifiers that colour or prioritise; `None` for informational messages.
    pub severity: Option<Severity>,
}

/// A one-way notification channel, for example a generic webhook.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Stable notifier id from config.
    fn id(&self) -> &str;

    /// Sends one message. Does not retry.
    async fn send(&self, msg: &OutboundMessage) -> Result<(), ChatError>;
}
