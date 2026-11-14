//! OpenAI-compatible Chat Completions: OpenAI, Ollama, LM Studio, OpenRouter and others.

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmCapability, LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role,
    StopReason,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::error::{LlmBuildError, excerpt};
use crate::http::{self, Reply};
use crate::openai_stream;
use crate::settings::Price;
use crate::usage;

/// A provider speaking the Chat Completions API. Local servers may need no key.
#[derive(Debug)]
pub struct OpenAiCompatible {
    client: reqwest::Client,
    url: String,
    model: String,
    api_key: Option<SecretString>,
    price: Option<Price>,
    /// Whether the server accepts `stream_options`. Some compatible servers reject it with
    /// a 400; once one does, streaming is dropped as well, because a streamed answer then
    /// arrives without usage totals. Non-streamed answers usually carry them.
    stream_options: AtomicBool,
    /// Whether answers are requested as a stream. Turned off with `stream_options`, so a
    /// server that cannot report usage in a stream is asked for a whole answer instead.
    streaming: AtomicBool,
    /// Whether the server accepts `tools`. Cleared when one rejects them, so the caller
    /// stops offering tools and gathers their output itself.
    tools: AtomicBool,
}

/// What one outgoing request actually carried, decided when its body was built. A rejection
/// is reported only when this request asked for the refused capability, even if another
/// request already withdrew it from the shared state.
#[derive(Debug, Clone, Copy)]
struct Included {
    tools: bool,
    stream_usage: bool,
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
            stream_options: AtomicBool::new(true),
            streaming: AtomicBool::new(true),
            tools: AtomicBool::new(true),
        })
    }

    /// Sends one prepared body and assembles its answer, streamed or whole.
    async fn send(
        &self,
        body: Value,
        secret: Option<&SecretString>,
    ) -> Result<WireResponse, LlmError> {
        let mut request = self.client.post(&self.url).json(&body);
        if let Some(key) = secret {
            request = request.bearer_auth(key.expose_secret());
        }
        match http::send_streaming::<WireResponse>(request, secret).await? {
            Reply::Whole(response) => Ok(response),
            Reply::Stream(mut events) => openai_stream::assemble(&mut events, secret).await,
        }
    }

    /// The request body, with the optional fields this server has not refused.
    fn body(&self, req: &LlmRequest) -> Value {
        let mut body = request_body(&self.model, req);
        let streaming = self.streaming.load(Ordering::Relaxed);
        body["stream"] = Value::Bool(streaming);
        if streaming && self.stream_options.load(Ordering::Relaxed) {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if (req.tools.is_empty() || !self.tools.load(Ordering::Relaxed))
            && let Some(fields) = body.as_object_mut()
        {
            fields.remove("tools");
        }
        body
    }

    /// What `body` actually carries, for telling whether a refusal is about this request.
    fn included(body: &Value) -> Included {
        Included {
            tools: body.get("tools").is_some(),
            stream_usage: body.get("stream_options").is_some(),
        }
    }

    /// Turns a specific capability refusal into the typed signal the engine acts on, after
    /// latching the capability off. The evidence is the request's own body, not the atomic
    /// transition: concurrent requests that both sent the field both report the refusal.
    /// Anything else is returned unchanged: an invalid model, a context window overflow or
    /// a bad tool schema is a plain error.
    fn refuse(&self, err: LlmError, included: Included) -> LlmError {
        let Some(capability) = refused_capability(&err) else {
            return err;
        };
        let carried = match capability {
            LlmCapability::Tools => included.tools,
            LlmCapability::StreamUsage => included.stream_usage,
        };
        if !carried {
            // This request already left the field out: the error is about something else.
            return err;
        }
        match capability {
            LlmCapability::Tools => {
                self.tools.store(false, Ordering::Relaxed);
            }
            LlmCapability::StreamUsage => {
                // Without streamed usage totals, ask for a whole answer: those usually
                // carry usage, so missing metadata does not hide what the turn cost.
                self.stream_options.store(false, Ordering::Relaxed);
                self.streaming.store(false, Ordering::Relaxed);
            }
        }
        tracing::warn!(
            capability = %capability,
            "the provider refused a capability; requests will leave it out",
        );
        LlmError::CapabilityRejected {
            capability,
            detail: err.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    fn id(&self) -> &str {
        "openai_compatible"
    }

    fn model(&self) -> Option<&str> {
        Some(self.model.as_ref())
    }

    fn supports_tools(&self) -> bool {
        self.tools.load(Ordering::Relaxed)
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        tracing::debug!(model = %self.model, messages = req.messages.len(), "openai turn");
        let secret = self.api_key.as_ref();
        let body = self.body(&req);
        let included = Self::included(&body);
        let response = match self.send(body, secret).await {
            Ok(response) => response,
            Err(err) => return Err(self.refuse(err, included)),
        };
        into_response(response, &req, self.price.as_ref(), self.api_key.as_ref())
    }
}

/// Words a server uses when it does not know a request field, as opposed to rejecting a
/// value of one. A capability is withdrawn only when one of these appears, so an invalid
/// model, a context overflow or a bad tool schema stays an ordinary error.
const REFUSAL_PHRASES: [&str; 10] = [
    "not supported",
    "unsupported",
    "does not support",
    "do not support",
    "not allowed",
    "not permitted",
    "unknown field",
    "unknown parameter",
    "unexpected",
    "unrecognized",
];

/// Words that name the tool capability itself, rather than a tool's schema or arguments.
const TOOL_FIELD_PHRASES: [&str; 11] = [
    "tool calling",
    "tool calls",
    "tool_calls",
    "tools are",
    "tools is",
    "tools not",
    "support tools",
    "support tool",
    "\"tools\"",
    "'tools'",
    "`tools`",
];

/// The capability a specific 4xx refusal is about, or `None` when the error is about
/// anything else. Only a refusal phrase together with the capability's own field counts.
fn refused_capability(err: &LlmError) -> Option<LlmCapability> {
    let LlmError::InvalidResponse(detail) = err else {
        return None;
    };
    // `Auth` covers 401 and 403, `RateLimited` covers 429; every other 4xx arrives here.
    if !detail.starts_with("HTTP 4") {
        return None;
    }
    let text = detail.to_ascii_lowercase();
    if !REFUSAL_PHRASES.iter().any(|phrase| text.contains(phrase)) {
        return None;
    }
    if text.contains("stream_options") || text.contains("include_usage") {
        return Some(LlmCapability::StreamUsage);
    }
    if TOOL_FIELD_PHRASES
        .iter()
        .any(|phrase| text.contains(phrase))
    {
        return Some(LlmCapability::Tools);
    }
    None
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
pub(crate) struct WireResponse {
    pub(crate) choices: Vec<WireChoice>,
    #[serde(default)]
    pub(crate) usage: Option<WireUsage>,
}

#[derive(Deserialize)]
pub(crate) struct WireChoice {
    pub(crate) message: WireMessage,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct WireMessage {
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
pub(crate) struct WireToolCall {
    pub(crate) id: String,
    pub(crate) function: WireFunction,
}

#[derive(Deserialize)]
pub(crate) struct WireFunction {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) arguments: String,
}

#[derive(Deserialize)]
pub(crate) struct WireUsage {
    #[serde(default)]
    pub(crate) prompt_tokens: u32,
    #[serde(default)]
    pub(crate) completion_tokens: u32,
}

fn into_response(
    response: WireResponse,
    req: &LlmRequest,
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
    // A server that reports no usage must not look free: infer the tokens from the request
    // and the answer, and price the estimate when the provider has a price.
    let usage = match response.usage {
        Some(reported) => {
            usage::reported(reported.prompt_tokens, reported.completion_tokens, price)
        }
        None => usage::estimated(
            usage::request_tokens(req),
            usage::answer_tokens(&parts),
            price,
        ),
    };
    Ok(LlmResponse { parts, stop, usage })
}
