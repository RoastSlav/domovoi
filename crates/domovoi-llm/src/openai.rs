//! OpenAI-compatible Chat Completions: OpenAI, Ollama, LM Studio, OpenRouter and others.

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, StopReason, Usage,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::error::{LlmBuildError, excerpt};
use crate::http;
use crate::settings::{Price, cost_micros};

/// A provider speaking the Chat Completions API. Local servers may need no key.
#[derive(Debug)]
pub struct OpenAiCompatible {
    client: reqwest::Client,
    url: String,
    model: String,
    api_key: Option<SecretString>,
    price: Option<Price>,
}

impl OpenAiCompatible {
    /// A provider for `model` at `base_url`, for example `http://localhost:11434/v1`.
    pub fn new(
        base_url: &Url,
        model: String,
        api_key: Option<SecretString>,
        price: Option<Price>,
        timeout: jiff::SignedDuration,
    ) -> Result<Self, LlmBuildError> {
        Ok(Self {
            client: http::client(timeout)?,
            url: http::endpoint(base_url, "chat/completions"),
            model,
            api_key,
            price,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    fn id(&self) -> &str {
        "openai_compatible"
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let body = request_body(&self.model, &req);
        tracing::debug!(model = %self.model, messages = req.messages.len(), "openai turn");
        let mut request = self.client.post(&self.url).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key.expose_secret());
        }
        let response: WireResponse = http::send(request, self.api_key.as_ref()).await?;
        into_response(response, self.price.as_ref(), self.api_key.as_ref())
    }
}

/// The JSON body for `req`, exposed for request-shape tests.
#[must_use]
pub fn request_body(model: &str, req: &LlmRequest) -> Value {
    let mut messages = Vec::new();
    if !req.system.is_empty() {
        messages.push(json!({ "role": "system", "content": req.system }));
    }
    for message in &req.messages {
        push_message(&mut messages, message);
    }
    let mut body = json!({
        "model": model,
        "max_tokens": req.max_output_tokens,
        "messages": messages,
    });
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
    }
    body
}

/// Adds `message` as Chat Completions messages. Tool results become `tool` messages, placed
/// first so they directly follow the assistant message that asked for them.
fn push_message(out: &mut Vec<Value>, message: &LlmMessage) {
    let text: Vec<&str> = message
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let text = (!text.is_empty()).then(|| text.join("\n\n"));
    match message.role {
        Role::User => {
            for part in &message.parts {
                if let Part::ToolResult {
                    id,
                    content,
                    is_error,
                } = part
                {
                    let content = if *is_error {
                        format!("error: {content}")
                    } else {
                        content.clone()
                    };
                    out.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
                }
            }
            if let Some(text) = text {
                out.push(json!({ "role": "user", "content": text }));
            }
        }
        Role::Assistant => {
            let calls: Vec<Value> = message
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::ToolCall { id, name, input } => Some(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": input.to_string() },
                    })),
                    _ => None,
                })
                .collect();
            let mut assistant = json!({ "role": "assistant", "content": text });
            if !calls.is_empty() {
                assistant["tool_calls"] = Value::Array(calls);
            }
            out.push(assistant);
        }
    }
}

#[derive(Deserialize)]
struct WireResponse {
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
struct WireToolCall {
    id: String,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

fn into_response(
    response: WireResponse,
    price: Option<&Price>,
    secret: Option<&SecretString>,
) -> Result<LlmResponse, LlmError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::InvalidResponse("the response has no choices".to_owned()))?;
    let mut parts = Vec::new();
    if let Some(text) = choice.message.content.filter(|t| !t.is_empty()) {
        parts.push(Part::Text(text));
    }
    for call in choice.message.tool_calls {
        let arguments = if call.function.arguments.trim().is_empty() {
            "{}"
        } else {
            call.function.arguments.as_str()
        };
        let input: Value = serde_json::from_str(arguments).map_err(|e| {
            LlmError::InvalidResponse(format!(
                "tool call {} has invalid JSON arguments ({e}): {}",
                excerpt(&call.function.name, secret),
                excerpt(arguments, secret)
            ))
        })?;
        parts.push(Part::ToolCall {
            id: call.id,
            name: call.function.name,
            input,
        });
    }
    let stop = match choice.finish_reason.as_deref() {
        Some("stop") | None => StopReason::EndTurn,
        Some("tool_calls" | "function_call") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some(other) => StopReason::Other(other.to_owned()),
    };
    let mut usage = response.usage.map_or_else(Usage::default, |u| Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        cost_micros: None,
    });
    usage.cost_micros = cost_micros(price, &usage);
    Ok(LlmResponse { parts, stop, usage })
}
