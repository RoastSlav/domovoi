//! A live publisher that records what it was told.

use std::sync::Mutex;

use domovoi_core::ConversationId;
use domovoi_core::ports::{LiveEvent, LivePublisher};

use crate::lock;

/// A [`LivePublisher`] that keeps every event, so a test can assert what a watcher
/// would have seen and in which order.
#[derive(Debug, Default)]
pub struct RecordingLive {
    events: Mutex<Vec<(ConversationId, LiveEvent)>>,
}

impl RecordingLive {
    /// An empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every event published so far, oldest first, with its conversation.
    #[must_use]
    pub fn events(&self) -> Vec<(ConversationId, LiveEvent)> {
        lock(&self.events).clone()
    }

    /// The events of one conversation, oldest first.
    #[must_use]
    pub fn events_of(&self, conversation: ConversationId) -> Vec<LiveEvent> {
        lock(&self.events)
            .iter()
            .filter(|(id, _)| *id == conversation)
            .map(|(_, event)| event.clone())
            .collect()
    }
}

impl LivePublisher for RecordingLive {
    fn publish(&self, conversation: ConversationId, event: LiveEvent) {
        lock(&self.events).push((conversation, event));
    }
}
