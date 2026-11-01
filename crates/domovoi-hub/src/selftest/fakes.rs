//! The offline selftest's stand-ins: a model that always returns one valid report and a
//! chat gateway that records cards instead of posting them. Never used by a real hub.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::Card;
use domovoi_core::ports::{
    ChatError, ChatGateway, InteractionSink, LlmError, LlmProvider, LlmRequest, LlmResponse,
    MessageRef, Part, StopReason, ThreadRef, Usage,
};
use tokio::sync::watch;

/// The report the scripted model returns.
pub(crate) const REPORT: &str = r#"{
  "headline": "Selftest incident",
  "summary": "A synthetic critical signal from domovoi selftest.",
  "cause": "domovoi selftest raised it on purpose.",
  "verdict": "EXPECTED",
  "next_step": "Nothing to do; it resolves by itself."
}"#;

/// Answers every request with [`REPORT`].
pub(crate) struct ScriptedProvider;

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn id(&self) -> &str {
        "selftest"
    }

    fn supports_tools(&self) -> bool {
        false
    }

    async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            parts: vec![Part::Text(REPORT.to_owned())],
            stop: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
}

/// Cards posted and updated so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) posted: usize,
    pub(crate) updated: usize,
}

/// Counts cards instead of posting them, and lets the selftest wait for a count.
pub(crate) struct RecordingGateway {
    counts: watch::Sender<Counts>,
}

impl Default for RecordingGateway {
    fn default() -> Self {
        Self {
            counts: watch::Sender::new(Counts::default()),
        }
    }
}

impl RecordingGateway {
    pub(crate) fn counts(&self) -> Counts {
        *self.counts.borrow()
    }

    /// Waits until `reached` holds for the counts.
    pub(crate) async fn wait(&self, reached: impl Fn(&Counts) -> bool) {
        let mut rx = self.counts.subscribe();
        // The sender lives as long as `self`, so this only ends early if it is dropped.
        let _ = rx.wait_for(|counts| reached(counts)).await;
    }
}

fn reference(n: usize) -> MessageRef {
    MessageRef {
        platform: "selftest".to_owned(),
        id: n.to_string(),
    }
}

#[async_trait]
impl ChatGateway for RecordingGateway {
    fn platform(&self) -> &str {
        "selftest"
    }

    async fn post_card(&self, _card: &Card) -> Result<MessageRef, ChatError> {
        let mut n = 0;
        self.counts.send_modify(|counts| {
            n = counts.posted;
            counts.posted += 1;
        });
        Ok(reference(n))
    }

    async fn update_card(&self, _msg: &MessageRef, _card: &Card) -> Result<(), ChatError> {
        self.counts.send_modify(|counts| counts.updated += 1);
        Ok(())
    }

    async fn open_thread(&self, msg: &MessageRef, _title: &str) -> Result<ThreadRef, ChatError> {
        Ok(ThreadRef {
            platform: msg.platform.clone(),
            id: msg.id.clone(),
        })
    }

    async fn reply(&self, _thread: &ThreadRef, _text: &str) -> Result<(), ChatError> {
        Ok(())
    }

    async fn run(&self, _sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        std::future::pending().await
    }
}
