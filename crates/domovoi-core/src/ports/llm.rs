//! One model turn with tools (ADR 0009).

use async_trait::async_trait;
use jiff::SignedDuration;
use serde_json::Value;

use crate::meta::MetaTool;
use crate::triage::ReadTool;

/// A tool the model may call, described for the provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// Tool name the model calls.
    pub name: String,
    /// What the tool does.
    pub description: String,
    /// JSON Schema of the tool's input object.
    pub input_schema: Value,
}

impl From<ReadTool> for ToolSpec {
    fn from(tool: ReadTool) -> Self {
        Self {
            name: tool.name().to_owned(),
            description: tool.description().to_owned(),
            input_schema: tool.json_schema(),
        }
    }
}

impl From<MetaTool> for ToolSpec {
    fn from(tool: MetaTool) -> Self {
        Self {
            name: tool.name().to_owned(),
            description: tool.description().to_owned(),
            input_schema: tool.json_schema(),
        }
    }
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Domovoi, including tool results.
    User,
    /// The model.
    Assistant,
}

/// One piece of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// Plain text.
    Text(String),
    /// The model asks to call a tool. The input is untrusted and parsed with
    /// [`ReadToolCall::from_json`](crate::triage::ReadToolCall::from_json).
    ToolCall {
        /// Provider-assigned call id, echoed in the result.
        id: String,
        /// Tool name.
        name: String,
        /// Raw arguments.
        input: Value,
    },
    /// The result of a tool call, sent back to the model.
    ToolResult {
        /// The call id this answers.
        id: String,
        /// Tool output, already redacted.
        content: String,
        /// Whether the tool failed.
        is_error: bool,
    },
}

/// One message in the conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmMessage {
    /// Who wrote it.
    pub role: Role,
    /// Its parts, in order.
    pub parts: Vec<Part>,
}

/// Everything the provider needs for one turn.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    /// System prompt.
    pub system: String,
    /// The conversation so far, oldest first.
    pub messages: Vec<LlmMessage>,
    /// Tools offered for this turn; empty when the provider does not support tools.
    pub tools: Vec<ToolSpec>,
    /// Cap on generated tokens.
    pub max_output_tokens: u32,
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished its answer.
    EndTurn,
    /// The model wants tool results before continuing.
    ToolUse,
    /// The output token cap was hit.
    MaxTokens,
    /// A provider-specific reason.
    Other(String),
}

/// Whether a turn's usage totals are real or inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageSource {
    /// The provider reported the totals.
    #[default]
    Reported,
    /// The provider reported nothing; the engine inferred the tokens from the request and
    /// the answer. Cost is only known when the provider has a price.
    Estimated,
    /// Nothing is known: neither the provider nor an inference produced totals.
    Unknown,
}

impl UsageSource {
    /// The stored name, `reported`, `estimated` or `unknown`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Estimated => "estimated",
            Self::Unknown => "unknown",
        }
    }
}

/// Tokens used by one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Prompt tokens.
    pub input_tokens: u32,
    /// Generated tokens.
    pub output_tokens: u32,
    /// Cost in millionths of a US dollar, when the provider reports or implies it.
    pub cost_micros: Option<u64>,
    /// Whether the totals were reported, inferred or are unknown.
    pub source: UsageSource,
}

impl Default for Usage {
    /// No tokens at a known zero cost: a caller that does not track usage, such as a fake
    /// or a provider that charges nothing, says so explicitly rather than leaving the cost
    /// unknown.
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cost_micros: Some(0),
            source: UsageSource::Reported,
        }
    }
}

/// The model's answer to one turn.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    /// Text and tool calls, in order.
    pub parts: Vec<Part>,
    /// Why it stopped.
    pub stop: StopReason,
    /// Token usage.
    pub usage: Usage,
}

/// A provider feature one request can leave out.
///
/// An adapter reports one through [`LlmError::CapabilityRejected`] when a provider refuses
/// it, so the caller can rebuild its request and prompt without it. Only capabilities whose
/// absence the caller can compensate for are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlmCapability {
    /// The `tools` field: the model calls read tools itself. Without it the caller gathers
    /// the tool output and puts it in the prompt.
    Tools,
    /// Usage totals inside a streamed answer (`stream_options`). Without it the answer still
    /// arrives, but its cost is unknown.
    StreamUsage,
}

impl std::fmt::Display for LlmCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tools => "tool calling",
            Self::StreamUsage => "streamed usage totals",
        })
    }
}

/// Why a turn failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    /// Network or protocol failure; retrying may help.
    #[error("transport error: {0}")]
    Transport(String),
    /// The provider asked to slow down.
    #[error("rate limited")]
    RateLimited {
        /// How long to wait, when the provider said.
        retry_after: Option<SignedDuration>,
    },
    /// The credentials were refused; retrying will not help.
    #[error("authentication failed")]
    Auth,
    /// The call did not finish within the hard per-call deadline.
    #[error("no answer within {after}")]
    Timeout {
        /// How long the call was allowed to take.
        after: SignedDuration,
    },
    /// The provider answered with something that could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    /// The provider refused a capability this request used. The same request without it may
    /// work, so the caller rebuilds what it sends and tries again. This is not an
    /// authentication or rate-limit failure.
    #[error("the provider refused {capability}: {detail}")]
    CapabilityRejected {
        /// What the provider refused.
        capability: LlmCapability,
        /// The provider's message, already redacted.
        detail: String,
    },
    /// The provider is not usable (disabled, missing binary, exhausted budget).
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// Whether the engine is starting or finishing a read tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPhase {
    /// The tool call begins.
    Started,
    /// The tool call ended.
    Finished,
}

/// Receives what a model turn produces while it runs. Used by the Operator to stream an
/// answer; everything else passes none.
pub trait AnswerSink: Send + Sync {
    /// A new model request starts: anything pushed before belonged to an attempt that was
    /// discarded.
    fn attempt_started(&self);
    /// A fragment of the text the model is writing in this attempt. Not redacted; the sink
    /// does that.
    fn text(&self, fragment: &str);
    /// The engine starts or finishes one read tool; `tool` is a name from the closed set or
    /// `unknown tool`.
    fn tool(&self, tool: &str, phase: ToolPhase, failed: bool);
}

/// A language model provider. One call is one model turn; the engine runs the tool loop.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable provider id used in config and usage records, for example `anthropic`.
    fn id(&self) -> &str;

    /// Model this provider answers with, when the adapter knows it; used in fallback
    /// log lines and failure text.
    fn model(&self) -> Option<&str> {
        None
    }

    /// Whether the provider accepts [`LlmRequest::tools`]. When `false` the engine sends no
    /// tools and expects a final answer.
    fn supports_tools(&self) -> bool;

    /// Whether this adapter can price a turn, so a monthly budget in dollars can count it.
    ///
    /// An adapter with no configured price returns `false`; a provider configured as
    /// explicitly free returns `true`, because zero is a price. Adapters whose turns are
    /// always free may keep the default.
    fn prices_usage(&self) -> bool {
        true
    }

    /// Runs one turn. Does not retry; retries and budgets are the engine's job.
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Runs one turn and hands text to `sink` as it is written. The default emits each text
    /// part once, after the turn, so a provider that cannot stream still works.
    async fn complete_streaming(
        &self,
        req: LlmRequest,
        sink: &dyn AnswerSink,
    ) -> Result<LlmResponse, LlmError> {
        let response = self.complete(req).await?;
        for part in &response.parts {
            if let Part::Text(text) = part {
                sink.text(text);
            }
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Plain;

    #[async_trait]
    impl LlmProvider for Plain {
        fn id(&self) -> &str {
            "plain"
        }
        fn supports_tools(&self) -> bool {
            false
        }
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                parts: vec![
                    Part::Text("one".to_owned()),
                    Part::ToolCall {
                        id: "t".to_owned(),
                        name: "journal".to_owned(),
                        input: Value::Null,
                    },
                    Part::Text("two".to_owned()),
                ],
                stop: StopReason::EndTurn,
                usage: Usage::default(),
            })
        }
    }

    #[derive(Default)]
    struct Collect(Mutex<Vec<String>>);

    impl AnswerSink for Collect {
        fn attempt_started(&self) {}
        fn text(&self, fragment: &str) {
            self.0.lock().unwrap().push(fragment.to_owned());
        }
        fn tool(&self, _tool: &str, _phase: ToolPhase, _failed: bool) {}
    }

    #[test]
    fn default_streaming_emits_the_whole_text_once() {
        let req = LlmRequest {
            system: String::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_output_tokens: 10,
        };
        let sink = Collect::default();
        let direct = futures::executor::block_on(Plain.complete(req.clone())).unwrap();
        let streamed = futures::executor::block_on(Plain.complete_streaming(req, &sink)).unwrap();
        assert_eq!(direct, streamed);
        assert_eq!(*sink.0.lock().unwrap(), ["one", "two"]);
    }
}
