//! Usage totals when a provider does not report them: the same shape as a reported total,
//! marked as estimated and priced when the provider has a price.

use domovoi_core::ports::{LlmRequest, Part, Usage, UsageSource};

use crate::settings::{Price, cost_micros};

/// Tokens in `text`, at the usual four characters per token, and never zero for text that
/// has any.
pub(crate) fn text_tokens(text: &str) -> u32 {
    u32::try_from(text.len().div_ceil(4))
        .unwrap_or(u32::MAX)
        .max(u32::from(!text.is_empty()))
}

/// Roughly how many prompt tokens `req` carries.
pub(crate) fn request_tokens(req: &LlmRequest) -> u32 {
    let mut tokens = text_tokens(&req.system);
    for message in &req.messages {
        for part in &message.parts {
            tokens = tokens.saturating_add(match part {
                Part::Text(text) => text_tokens(text),
                Part::ToolResult { content, .. } => text_tokens(content),
                Part::ToolCall { name, input, .. } => {
                    text_tokens(name).saturating_add(text_tokens(&input.to_string()))
                }
            });
        }
    }
    for tool in &req.tools {
        tokens = tokens
            .saturating_add(text_tokens(&tool.name))
            .saturating_add(text_tokens(&tool.description))
            .saturating_add(text_tokens(&tool.input_schema.to_string()));
    }
    tokens
}

/// Roughly how many tokens the answer's parts are.
pub(crate) fn answer_tokens(parts: &[Part]) -> u32 {
    parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => text_tokens(text),
            Part::ToolCall { name, input, .. } => {
                text_tokens(name).saturating_add(text_tokens(&input.to_string()))
            }
            Part::ToolResult { content, .. } => text_tokens(content),
        })
        .fold(0u32, u32::saturating_add)
}

/// A usage total this crate inferred, priced when `price` is known.
pub(crate) fn estimated(input_tokens: u32, output_tokens: u32, price: Option<&Price>) -> Usage {
    let mut usage = Usage {
        input_tokens,
        output_tokens,
        cost_micros: None,
        source: UsageSource::Estimated,
    };
    usage.cost_micros = cost_micros(price, &usage);
    usage
}

/// A usage total the provider reported, priced when `price` is known.
pub(crate) fn reported(input_tokens: u32, output_tokens: u32, price: Option<&Price>) -> Usage {
    let mut usage = Usage {
        input_tokens,
        output_tokens,
        cost_micros: None,
        source: UsageSource::Reported,
    };
    usage.cost_micros = cost_micros(price, &usage);
    usage
}
