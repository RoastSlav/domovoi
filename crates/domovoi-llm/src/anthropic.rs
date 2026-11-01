//! The Anthropic Messages API.

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, StopReason, Usage,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::error::LlmBuildError;
use crate::http;
use crate::settings::{Price, cost_micros};

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

    fn supports_tools(&self) -> bool {
        true
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let body = request_body(&self.model, &req);
        tracing::debug!(model = %self.model, messages = req.messages.len(), "anthropic turn");
        let request = self
            .client
            .post(&self.url)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .json(&body);
        let response: WireResponse = http::send(request, Some(&self.api_key)).await?;
        Ok(into_response(response, self.price.as_ref()))
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
struct WireResponse {
    content: Vec<WireContent>,
    stop_reason: Option<String>,
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Default)]
struct WireUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

fn into_response(response: WireResponse, price: Option<&Price>) -> LlmResponse {
    let parts = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            WireContent::Text { text } => Some(Part::Text(text)),
            WireContent::ToolUse { id, name, input } => Some(Part::ToolCall { id, name, input }),
            WireContent::Other => None,
        })
        .collect();
    let stop = match response.stop_reason.as_deref() {
        Some("end_turn" | "stop_sequence") | None => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some(other) => StopReason::Other(other.to_owned()),
    };
    let mut usage = Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_micros: None,
    };
    usage.cost_micros = cost_micros(price, &usage);
    LlmResponse { parts, stop, usage }
}
