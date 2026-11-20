//! The live bus: ephemeral events of an answer being written, fanned out to event streams.
//!
//! Delivery is lossy by design. The bus never blocks the publisher; a stream that cannot keep
//! up misses frames and is repaired by the next snapshot or by the committed message.

use std::sync::Arc;

use domovoi_core::ConversationId;
use domovoi_core::ports::{LiveEnd, LiveEvent, LivePublisher, ToolPhase};
use serde_json::json;
use tokio::sync::broadcast;

/// How many frames the bus holds for a slow stream before it starts dropping.
pub const LIVE_CAPACITY: usize = 256;

/// One live event, ready to be written as a server-sent event.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveFrame {
    /// The conversation it belongs to, as a UUID string.
    pub conversation: String,
    /// The event name: `live.start`, `live.text`, `live.tool` or `live.end`.
    pub name: &'static str,
    /// The JSON data line.
    pub data: Arc<str>,
}

impl LiveFrame {
    /// The frame for `event` of `conversation`.
    #[must_use]
    pub fn of(conversation: ConversationId, event: &LiveEvent) -> Self {
        let id = conversation.to_string();
        let (name, data) = match event {
            LiveEvent::Start { turn, message } => (
                "live.start",
                json!({"conversation": id, "turn": turn.to_string(), "message": message.to_string()}),
            ),
            LiveEvent::Text {
                turn,
                attempt,
                text,
            } => (
                "live.text",
                json!({"conversation": id, "turn": turn.to_string(), "attempt": attempt, "text": text}),
            ),
            LiveEvent::Tool {
                turn,
                tool,
                phase,
                failed,
            } => (
                "live.tool",
                json!({
                    "conversation": id,
                    "turn": turn.to_string(),
                    "tool": tool,
                    "state": match phase {
                        ToolPhase::Started => "started",
                        ToolPhase::Finished => "finished",
                    },
                    "failed": failed,
                }),
            ),
            LiveEvent::End { turn, outcome } => (
                "live.end",
                json!({
                    "conversation": id,
                    "turn": turn.to_string(),
                    "status": match outcome {
                        LiveEnd::Complete => "complete",
                        LiveEnd::Failed => "failed",
                        LiveEnd::Cancelled => "cancelled",
                        LiveEnd::Discarded => "discarded",
                    },
                }),
            ),
        };
        Self {
            conversation: id,
            name,
            data: Arc::from(data.to_string()),
        }
    }
}

/// The hub-wide fan-out of live frames.
pub struct LiveBus {
    tx: broadcast::Sender<LiveFrame>,
}

impl LiveBus {
    /// An empty bus.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(LIVE_CAPACITY);
        Arc::new(Self { tx })
    }

    /// A receiver of every frame published from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<LiveFrame> {
        self.tx.subscribe()
    }
}

impl LivePublisher for LiveBus {
    fn publish(&self, conversation: ConversationId, event: LiveEvent) {
        // No receivers is the normal case when nobody watches; the frame is simply dropped.
        let _ = self.tx.send(LiveFrame::of(conversation, &event));
    }
}
