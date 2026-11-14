//! Usage totals when a provider does not report them: the same shape as a reported total,
//! marked as estimated and priced when the provider has a price.

use domovoi_core::ports::{LlmRequest, Part, StopReason, Usage, UsageSource};

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

/// What an answer contains, for estimating its usage when the provider reported none.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Answer<'a> {
    /// Visible text and tool calls.
    pub(crate) parts: &'a [Part],
    /// Reasoning the provider exposed; redacted reasoning has no text.
    pub(crate) reasoning: &'a str,
    /// Why the model stopped.
    pub(crate) stop: &'a StopReason,
    /// The output allowance the request authorised.
    pub(crate) allowance: u32,
    /// Whether some reasoning was redacted, so visible text is not the whole output.
    pub(crate) redacted_thinking: bool,
}

/// The usage of an answer whose provider reported none.
///
/// The visible text, the tool calls and the reasoning a provider exposed are counted. A turn
/// that ended at its output allowance used the whole allowance, and hidden reasoning may
/// account for all of it, so the allowance is the only safe number. The same holds when the
/// provider says part of its reasoning is redacted: its text cannot be counted, so the
/// visible answer alone would understate the turn. When nothing at all can be counted, the
/// request was still authorised and may have generated hidden output, so the allowance is
/// charged as a conservative bound and the total stays [`UsageSource::Unknown`] — never a
/// free turn.
pub(crate) fn estimated_answer(
    input_tokens: u32,
    answer: &Answer<'_>,
    price: Option<&Price>,
) -> Usage {
    let output = if answer.redacted_thinking || *answer.stop == StopReason::MaxTokens {
        answer.allowance
    } else {
        answer_tokens(answer.parts).saturating_add(text_tokens(answer.reasoning))
    };
    if output == 0 {
        let mut usage = Usage {
            input_tokens,
            output_tokens: answer.allowance,
            cost_micros: None,
            source: UsageSource::Unknown,
        };
        usage.cost_micros = cost_micros(price, &usage);
        return usage;
    }
    estimated(input_tokens, output, price)
}

/// The usage of a response that reported only part of its totals, or none at all.
///
/// A reported half is kept — including an explicit zero — and only the missing half is
/// estimated. Both present is a complete report and belongs to [`reported`] instead.
pub(crate) fn partial_or_estimated(
    reported_input: Option<u32>,
    reported_output: Option<u32>,
    request_input: u32,
    answer: &Answer<'_>,
    price: Option<&Price>,
) -> Usage {
    let input = reported_input.unwrap_or(request_input);
    match reported_output {
        Some(output) => estimated(input, output, price),
        None => estimated_answer(input, answer, price),
    }
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
