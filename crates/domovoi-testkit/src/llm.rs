//! A language model that plays back a script.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmError, LlmProvider, LlmRequest, LlmResponse, Part, StopReason, Usage,
};

use crate::lock;

/// An [`LlmProvider`] that answers from a queue and records every request.
///
/// When the queue is empty it returns `LlmError::Unavailable("script exhausted")`.
#[derive(Debug)]
pub struct ScriptedLlm {
    id: String,
    supports_tools: bool,
    script: Mutex<VecDeque<Result<LlmResponse, LlmError>>>,
    requests: Mutex<Vec<LlmRequest>>,
}

impl ScriptedLlm {
    /// An empty script for provider `scripted` that supports tools.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: "scripted".to_owned(),
            supports_tools: true,
            script: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Makes the provider report that it does not support tools.
    #[must_use]
    pub fn without_tools(mut self) -> Self {
        self.supports_tools = false;
        self
    }

    /// Queues a response.
    pub fn push_ok(&self, response: LlmResponse) {
        lock(&self.script).push_back(Ok(response));
    }

    /// Queues a final text answer with zero usage.
    pub fn push_text(&self, text: impl Into<String>) {
        self.push_ok(LlmResponse {
            parts: vec![Part::Text(text.into())],
            stop: StopReason::EndTurn,
            usage: Usage::default(),
        });
    }

    /// Queues an error.
    pub fn push_err(&self, error: LlmError) {
        lock(&self.script).push_back(Err(error));
    }

    /// Every request received so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LlmRequest> {
        lock(&self.requests).clone()
    }

    /// How many scripted answers are left.
    #[must_use]
    pub fn remaining(&self) -> usize {
        lock(&self.script).len()
    }
}

impl Default for ScriptedLlm {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
    fn id(&self) -> &str {
        &self.id
    }

    fn supports_tools(&self) -> bool {
        self.supports_tools
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        lock(&self.requests).push(req);
        lock(&self.script)
            .pop_front()
            .unwrap_or_else(|| Err(LlmError::Unavailable("script exhausted".to_owned())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(system: &str) -> LlmRequest {
        LlmRequest {
            system: system.to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_output_tokens: 100,
        }
    }

    #[tokio::test]
    async fn scripted_llm_returns_in_order_and_records_requests() {
        let llm = ScriptedLlm::new();
        llm.push_text("first");
        llm.push_err(LlmError::Auth);
        llm.push_text("third");

        let first = llm.complete(request("a")).await.unwrap();
        assert_eq!(first.parts, vec![Part::Text("first".to_owned())]);
        assert_eq!(llm.complete(request("b")).await, Err(LlmError::Auth));
        let third = llm.complete(request("c")).await.unwrap();
        assert_eq!(third.parts, vec![Part::Text("third".to_owned())]);

        let systems: Vec<String> = llm.requests().into_iter().map(|r| r.system).collect();
        assert_eq!(systems, ["a", "b", "c"]);
        assert_eq!(llm.remaining(), 0);
    }

    #[tokio::test]
    async fn scripted_llm_errors_when_exhausted() {
        let llm = ScriptedLlm::new();
        assert_eq!(
            llm.complete(request("a")).await,
            Err(LlmError::Unavailable("script exhausted".to_owned()))
        );
        assert_eq!(llm.requests().len(), 1);
    }
}
