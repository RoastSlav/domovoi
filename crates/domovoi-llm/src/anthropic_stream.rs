//! Assembling a Messages API event stream into the one response `complete` returns.

use std::collections::BTreeMap;

use domovoi_core::ports::LlmError;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::Value;

use crate::anthropic::{WireContent, WireResponse, WireUsage};
use crate::error::excerpt;
use crate::sse::Events;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    MessageStart {
        message: StartMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: StartBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: BlockDelta,
    },
    MessageDelta {
        delta: MessageDelta,
        #[serde(default)]
        usage: Option<DeltaUsage>,
    },
    MessageStop,
    Error {
        error: ErrorBody,
    },
    /// `ping`, `content_block_stop` and event types added later.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct StartMessage {
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StartBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BlockDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct MessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct DeltaUsage {
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
}

enum Block {
    Text(String),
    Tool {
        id: String,
        name: String,
        json: String,
    },
    Other,
}

/// Reads `events` to the end and returns what the model said. A stream that stops before
/// `message_stop` was cut off and is an error.
pub(crate) async fn assemble(
    events: &mut Events,
    secret: Option<&SecretString>,
) -> Result<WireResponse, LlmError> {
    let mut blocks: BTreeMap<usize, Block> = BTreeMap::new();
    let mut stop_reason = None;
    let mut usage = WireUsage::default();
    let mut complete = false;
    while let Some(data) = events.next_data(secret).await? {
        let event: Event = serde_json::from_str(&data)
            .map_err(|e| LlmError::InvalidResponse(format!("{e}: {}", excerpt(&data, secret))))?;
        match event {
            Event::MessageStart { message } => usage = message.usage,
            Event::ContentBlockStart {
                index,
                content_block,
            } => {
                let block = match content_block {
                    StartBlock::Text { text } => Block::Text(text),
                    StartBlock::ToolUse { id, name } => Block::Tool {
                        id,
                        name,
                        json: String::new(),
                    },
                    StartBlock::Other => Block::Other,
                };
                blocks.insert(index, block);
            }
            Event::ContentBlockDelta { index, delta } => match (blocks.get_mut(&index), delta) {
                (Some(Block::Text(text)), BlockDelta::TextDelta { text: more }) => {
                    text.push_str(&more);
                }
                (Some(Block::Tool { json, .. }), BlockDelta::InputJsonDelta { partial_json }) => {
                    json.push_str(&partial_json);
                }
                _ => {}
            },
            Event::MessageDelta {
                delta,
                usage: delta_usage,
            } => {
                if delta.stop_reason.is_some() {
                    stop_reason = delta.stop_reason;
                }
                if let Some(delta_usage) = delta_usage {
                    usage.output_tokens = delta_usage.output_tokens;
                }
            }
            Event::MessageStop => {
                complete = true;
                break;
            }
            Event::Error { error } => return Err(stream_error(&error, secret)),
            Event::Other => {}
        }
    }
    if !complete {
        return Err(LlmError::Unavailable(
            "the stream ended before the model finished".to_owned(),
        ));
    }
    let content = blocks
        .into_values()
        .filter_map(|block| block_content(block, secret).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WireResponse {
        content,
        stop_reason,
        usage,
    })
}

fn block_content(
    block: Block,
    secret: Option<&SecretString>,
) -> Result<Option<WireContent>, LlmError> {
    Ok(match block {
        Block::Text(text) => Some(WireContent::Text { text }),
        Block::Tool { id, name, json } => {
            let json = if json.trim().is_empty() { "{}" } else { &json };
            let input: Value = serde_json::from_str(json).map_err(|e| {
                LlmError::InvalidResponse(format!(
                    "tool call {} has invalid JSON input ({e}): {}",
                    excerpt(&name, secret),
                    excerpt(json, secret)
                ))
            })?;
            Some(WireContent::ToolUse { id, name, input })
        }
        Block::Other => None,
    })
}

fn stream_error(error: &ErrorBody, secret: Option<&SecretString>) -> LlmError {
    let text = excerpt(&error.message, secret);
    match error.kind.as_str() {
        "authentication_error" | "permission_error" => LlmError::Auth,
        "rate_limit_error" => LlmError::RateLimited { retry_after: None },
        "overloaded_error" | "api_error" | "timeout_error" => {
            LlmError::Unavailable(format!("{}: {text}", error.kind))
        }
        other => LlmError::InvalidResponse(format!("{other}: {text}")),
    }
}
