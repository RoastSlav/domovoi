//! A language model that plays back a script.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmError, LlmProvider, LlmRequest, LlmResponse, Part, StopReason, Usage,
};
use tokio::sync::Semaphore;

use crate::lock;

/// An [`LlmProvider`] that answers from a queue and records every request.
///
/// When the queue is empty it returns `LlmError::Unavailable("script exhausted")`.
/// [`hold`](Self::hold) can make the next response wait, so a test can run on while a
/// request is inside the provider, and [`wait_for_request`](Self::wait_for_request)
/// waits until one has actually arrived.
#[derive(Debug)]
pub struct ScriptedLlm {
    id: String,
    supports_tools: bool,
    script: Mutex<VecDeque<Result<LlmResponse, LlmError>>>,
    requests: Mutex<Vec<LlmRequest>>,
    /// Permits a [`complete`] must hold before taking its scripted response.
    gate: Semaphore,
    /// One permit fired per received request, for [`wait_for_request`].
    arrived: Semaphore,
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
            gate: Semaphore::new(1024),
            arrived: Semaphore::new(0),
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

    /// Takes the next scripted response only after [`release`](Self::release) hands
    /// the permits back, so a test can work on while the request is already inside
    /// the provider. Call it before spawning the request, with at most one request
    /// blocked at a time.
    pub fn hold(&self) {
        self.gate.forget_permits(self.gate.available_permits());
    }

    /// Hands `n` permits back, letting up to `n` blocked requests take their response.
    pub fn release(&self, n: usize) {
        self.gate.add_permits(n);
    }

    /// Waits until the provider has received the next request. When this returns the
    /// request is already recorded in [`requests`](Self::requests) and its provider is
    /// holding it, which also proves anything the caller read before the call, such as
    /// a conversation snapshot, happened before the response could arrive. One wait
    /// consumes one arrival signal.
    pub async fn wait_for_request(&self) {
        let permit = self.arrived.acquire().await;
        match permit {
            Ok(held) => held.forget(),
            Err(_closed) => {}
        }
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
        // The request counts as received before the gate is taken, so a test can
        // `wait_for_request` even while [`hold`](Self::hold) blocks the response.
        self.arrived.add_permits(1);
        match self.gate.acquire().await {
            Ok(_held) => {}
            Err(_closed) => {}
        }
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
