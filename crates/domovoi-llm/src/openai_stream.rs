//! Assembling a Chat Completions event stream into the one response `complete` returns.

use std::collections::BTreeMap;

use domovoi_core::ports::LlmError;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::Value;

use crate::error::excerpt;
use crate::openai::{WireChoice, WireFunction, WireMessage, WireResponse, WireToolCall, WireUsage};
use crate::sse::Events;

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<DeltaCall>,
}

#[derive(Deserialize)]
struct DeltaCall {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// One tool call as its pieces arrive.
#[derive(Default)]
struct Partial {
    id: String,
    name: String,
    arguments: String,
}

/// Reads `events` to the end and returns what the model said. A stream that stops before
/// a finish reason or `[DONE]` was cut off and is an error.
pub(crate) async fn assemble(
    events: &mut Events,
    secret: Option<&SecretString>,
) -> Result<WireResponse, LlmError> {
    let mut content = String::new();
    let mut calls: BTreeMap<usize, Partial> = BTreeMap::new();
    let mut finish_reason = None;
    let mut usage = None;
    let mut complete = false;
    while let Some(data) = events.next_data(secret).await? {
        if data.trim() == "[DONE]" {
            complete = true;
            break;
        }
        let chunk: Chunk = serde_json::from_str(&data)
            .map_err(|e| LlmError::InvalidResponse(format!("{e}: {}", excerpt(&data, secret))))?;
        if let Some(error) = chunk.error {
            return Err(LlmError::Unavailable(format!(
                "the stream reported an error: {}",
                excerpt(&error.to_string(), secret)
            )));
        }
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
        for choice in chunk.choices {
            if let Some(text) = choice.delta.content {
                content.push_str(&text);
            }
            for call in choice.delta.tool_calls {
                add_call(&mut calls, call);
            }
            if choice.finish_reason.is_some() {
                finish_reason = choice.finish_reason;
                complete = true;
            }
        }
    }
    if !complete {
        return Err(LlmError::Unavailable(
            "the stream ended before the model finished".to_owned(),
        ));
    }
    Ok(WireResponse {
        choices: vec![WireChoice {
            message: WireMessage {
                content: (!content.is_empty()).then_some(content),
                tool_calls: calls
                    .into_values()
                    .map(|call| WireToolCall {
                        id: call.id,
                        function: WireFunction {
                            name: call.name,
                            arguments: call.arguments,
                        },
                    })
                    .collect(),
            },
            finish_reason,
        }],
        usage,
    })
}

/// Adds a fragment to its call. Some servers omit `index`, so a fragment that names a
/// different id than the call at its index starts a new call.
fn add_call(calls: &mut BTreeMap<usize, Partial>, fragment: DeltaCall) {
    let mut index = fragment.index;
    if let (Some(id), Some(existing)) = (&fragment.id, calls.get(&index))
        && !existing.id.is_empty()
        && existing.id != *id
    {
        index = calls.keys().next_back().map_or(0, |last| last + 1);
    }
    let call = calls.entry(index).or_default();
    if let Some(id) = fragment.id {
        call.id = id;
    }
    if let Some(function) = fragment.function {
        if let Some(name) = function.name {
            call.name.push_str(&name);
        }
        if let Some(arguments) = function.arguments {
            call.arguments.push_str(&arguments);
        }
    }
}
