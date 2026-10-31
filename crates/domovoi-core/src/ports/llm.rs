//! One model turn with tools (ADR 0009).

use async_trait::async_trait;
use jiff::SignedDuration;
use serde_json::Value;

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

/// Tokens used by one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    /// Prompt tokens.
    pub input_tokens: u32,
    /// Generated tokens.
    pub output_tokens: u32,
    /// Cost in millionths of a US dollar, when the provider reports or implies it.
    pub cost_micros: Option<u64>,
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
    /// The provider answered with something that could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    /// The provider is not usable (disabled, missing binary, exhausted budget).
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// A language model provider. One call is one model turn; the engine runs the tool loop.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable provider id used in config and usage records, for example `anthropic`.
    fn id(&self) -> &str;

    /// Whether the provider accepts [`LlmRequest::tools`]. When `false` the engine sends no
    /// tools and expects a final answer.
    fn supports_tools(&self) -> bool;

    /// Runs one turn. Does not retry; retries and budgets are the engine's job.
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
}
