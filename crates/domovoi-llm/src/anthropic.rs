//! The Anthropic Messages API.

use async_trait::async_trait;
use domovoi_core::ports::{
    AnswerSink, LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, StopReason,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::anthropic_stream;
use crate::error::LlmBuildError;
use crate::http::{self, Reply};
use crate::settings::Price;

/// Where the API lives when no base URL is configured.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// The API version every request names.
pub const API_VERSION: &str = "2023-06-01";

/// A provider speaking the Anthropic Messages API.
#[derive(Debug)]
pub struct Anthropic {
    client: reqwest::Client,
    url: String,
    model: String,
    api_key: SecretString,
    price: Option<Price>,
}

impl Anthropic {
    /// A provider for `model` at `base_url` (or [`DEFAULT_BASE_URL`]).
    pub fn new(
        base_url: Option<&Url>,
        model: String,
        api_key: SecretString,
        price: Option<Price>,
        timeout: jiff::SignedDuration,
    ) -> Result<Self, LlmBuildError> {
        let url = match base_url {
            Some(base) => http::endpoint(base, "v1/messages"),
            None => format!("{DEFAULT_BASE_URL}/v1/messages"),
        };
        Ok(Self {
            client: http::client(timeout)?,
            url,
            model,
            api_key,
            price,
        })
    }
}

#[async_trait]
impl LlmProvider for Anthropic {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> Option<&str> {
        Some(self.model.as_ref())
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn prices_usage(&self) -> bool {
        self.price.is_some()
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.turn(req, None).await
    }

    async fn complete_streaming(
        &self,
        req: LlmRequest,
        sink: &dyn AnswerSink,
    ) -> Result<LlmResponse, LlmError> {
        self.turn(req, Some(sink)).await
    }
}

impl Anthropic {
    /// One turn; text is forwarded to `sink` while it streams in.
    async fn turn(
        &self,
        req: LlmRequest,
        sink: Option<&dyn AnswerSink>,
    ) -> Result<LlmResponse, LlmError> {
        let mut body = request_body(&self.model, &req);
        body["stream"] = Value::Bool(true);
        tracing::debug!(model = %self.model, messages = req.messages.len(), "anthropic turn");
        let request = self
            .client
            .post(&self.url)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .json(&body);
        let secret = Some(&self.api_key);
        let (response, streamed) =
            match http::send_streaming::<WireResponse>(request, secret).await? {
                Reply::Whole(response) => (response, false),
                Reply::Stream(mut events) => (
                    anthropic_stream::assemble(&mut events, secret, sink).await?,
                    true,
                ),
            };
        let response = into_response(response, &req, self.price.as_ref());
        if let (Some(sink), false) = (sink, streamed) {
            emit_text(&response, sink);
        }
        Ok(response)
    }
}

/// Hands the text parts of a response that did not stream to `sink`, once each.
pub(crate) fn emit_text(response: &LlmResponse, sink: &dyn AnswerSink) {
    for part in &response.parts {
        if let Part::Text(text) = part {
            sink.text(text);
        }
    }
}

/// The JSON body for `req`, exposed for request-shape tests.
#[must_use]
pub fn request_body(model: &str, req: &LlmRequest) -> Value {
    let request = WireRequest {
        model,
        max_tokens: req.max_output_tokens,
        system: (!req.system.is_empty()).then_some(req.system.as_str()),
        messages: req.messages.iter().map(wire_message).collect(),
        tools: req
            .tools
            .iter()
            .map(|t| WireTool {
                name: &t.name,
                description: &t.description,
                input_schema: &t.input_schema,
            })
            .collect(),
    };
    serde_json::to_value(request).unwrap_or(Value::Null)
}

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool<'a>>,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'static str,
    content: Vec<WireBlock<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireBlock<'a> {
    Text {
        text: &'a str,
    },
    ToolUse {
        id: &'a str,
        name: &'a str,
        input: &'a Value,
    },
    ToolResult {
        tool_use_id: &'a str,
        content: &'a str,
        is_error: bool,
    },
}

#[derive(Serialize)]
struct WireTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a Value,
}

fn wire_message(message: &LlmMessage) -> WireMessage<'_> {
    WireMessage {
        role: match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        },
        content: message
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => WireBlock::Text { text },
                Part::ToolCall { id, name, input } => WireBlock::ToolUse { id, name, input },
                Part::ToolResult {
                    id,
                    content,
                    is_error,
                } => WireBlock::ToolResult {
                    tool_use_id: id,
                    content,
                    is_error: *is_error,
                },
            })
            .collect(),
    }
}

#[derive(Deserialize)]
pub(crate) struct WireResponse {
    pub(crate) content: Vec<WireContent>,
    pub(crate) stop_reason: Option<String>,
    #[serde(default)]
    pub(crate) usage: Option<WireUsage>,
    /// Input tokens a stream reported before output usage arrived, if any.
    #[serde(skip)]
    pub(crate) input_tokens: Option<u32>,
    /// Output tokens a stream reported even when the input total never arrived.
    #[serde(skip)]
    pub(crate) output_tokens: Option<u32>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WireContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Private reasoning. Used for usage estimation only, never returned as answer text.
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    /// Reasoning the provider encrypted. There is no text to count or expose; its presence
    /// alone says the turn generated hidden output. The payload is skipped, never stored.
    RedactedThinking {
        #[serde(skip)]
        _data: (),
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Default)]
pub(crate) struct WireUsage {
    /// Present when the provider reported the prompt total.
    #[serde(default)]
    pub(crate) input_tokens: Option<u32>,
    /// Present when the provider reported the generated total.
    #[serde(default)]
    pub(crate) output_tokens: Option<u32>,
}

fn into_response(response: WireResponse, req: &LlmRequest, price: Option<&Price>) -> LlmResponse {
    let mut parts = Vec::new();
    let mut reasoning = String::new();
    let mut redacted_thinking = false;
    for block in response.content {
        match block {
            WireContent::Text { text } => parts.push(Part::Text(text)),
            WireContent::ToolUse { id, name, input } => {
                parts.push(Part::ToolCall { id, name, input });
            }
            // Thinking is billed output, but it is private reasoning: count it, never show it.
            WireContent::Thinking { thinking } => reasoning.push_str(&thinking),
            // Nothing to count and nothing to show, but its presence means the visible text
            // is not the whole output.
            WireContent::RedactedThinking { .. } => redacted_thinking = true,
            WireContent::Other => {}
        }
    }
    let stop = match response.stop_reason.as_deref() {
        Some("end_turn" | "stop_sequence") | None => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some(other) => StopReason::Other(other.to_owned()),
    };
    // Complete reported totals are authoritative. A response that reported only one half
    // keeps it and has the other estimated; a response with no totals at all is estimated
    // from what it contains â€” or, with redacted reasoning, from the whole authorised
    // allowance.
    let answer = crate::usage::Answer {
        parts: &parts,
        reasoning: &reasoning,
        stop: &stop,
        allowance: req.max_output_tokens,
        redacted_thinking,
    };
    let (input, output) = match response.usage {
        Some(reported) => (
            reported.input_tokens.or(response.input_tokens),
            reported.output_tokens.or(response.output_tokens),
        ),
        None => (response.input_tokens, response.output_tokens),
    };
    let usage = match (input, output) {
        (Some(input), Some(output)) => crate::usage::reported(input, output, price),
        _ => crate::usage::partial_or_estimated(
            input,
            output,
            crate::usage::request_tokens(req),
            &answer,
            price,
        ),
    };
    LlmResponse { parts, stop, usage }
}
