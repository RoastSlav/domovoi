//! The offline selftest's stand-ins: a model that always returns one valid report and a
//! chat gateway that records cards instead of posting them. Never used by a real hub.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use domovoi_core::Card;
use domovoi_core::ports::{
    ChatError, ChatGateway, InteractionSink, LlmError, LlmProvider, LlmRequest, LlmResponse,
    MessageRef, Part, StopReason, ThreadRef, Usage,
};

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

/// Counts cards instead of posting them.
#[derive(Default)]
pub(crate) struct RecordingGateway {
    posted: AtomicUsize,
    updated: AtomicUsize,
}

impl RecordingGateway {
    pub(crate) fn posted(&self) -> usize {
        self.posted.load(Ordering::SeqCst)
    }

    pub(crate) fn updated(&self) -> usize {
        self.updated.load(Ordering::SeqCst)
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
        Ok(reference(self.posted.fetch_add(1, Ordering::SeqCst)))
    }

    async fn update_card(&self, _msg: &MessageRef, _card: &Card) -> Result<(), ChatError> {
        self.updated.fetch_add(1, Ordering::SeqCst);
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
