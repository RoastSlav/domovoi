//! Prompt sizing, the per-call byte cap and failure-text redaction.

use domovoi_core::ports::{LlmError, LlmRequest, Part, Redactor};

use super::tools;

/// What replaces an old tool result that had to go to fit the prompt cap.
const OMITTED_TOOL_RESULT: &str = "[earlier tool output omitted to fit the prompt size limit]";

/// The smallest a tool result may be cut to in the newest message, in bytes.
const MIN_TOOL_RESULT_BYTES: usize = 1024;

/// Removes registered secrets from the free text of a provider failure, before it is
/// logged, recorded or returned.
pub(crate) fn redact_error(redactor: &dyn Redactor, err: LlmError) -> LlmError {
    match err {
        LlmError::Transport(text) => LlmError::Transport(redactor.redact(&text)),
        LlmError::InvalidResponse(text) => LlmError::InvalidResponse(redactor.redact(&text)),
        LlmError::Unavailable(text) => LlmError::Unavailable(redactor.redact(&text)),
        LlmError::CapabilityRejected { capability, detail } => LlmError::CapabilityRejected {
            capability,
            detail: redactor.redact(&detail),
        },
        other => other,
    }
}

/// The bytes a prompt occupies: the system prompt and every part's text.
pub(crate) fn prompt_size(req: &LlmRequest) -> usize {
    let mut size = req.system.len();
    for message in &req.messages {
        for part in &message.parts {
            size += match part {
                Part::Text(text) => text.len(),
                Part::ToolCall { name, input, .. } => name.len() + input.to_string().len(),
                Part::ToolResult { content, .. } => content.len(),
            };
        }
    }
    size
}

/// Shortens `req` until its size fits `cap`, trimming old tool results first and the newest
/// ones after that. The first user message and the system prompt are never touched. The
/// second value says whether anything had to be trimmed.
pub(crate) fn fit_to_cap(req: LlmRequest, cap: usize) -> Result<(LlmRequest, bool), LlmError> {
    if prompt_size(&req) <= cap {
        return Ok((req, false));
    }
    let mut req = req;
    // Step A: old tool results become a placeholder, oldest first. The id and error flag
    // stay, so the provider's tool pairing remains valid.
    let old_count = req.messages.len().saturating_sub(1);
    'old: for message in 0..old_count {
        for index in 0..req.messages[message].parts.len() {
            let replaceable = matches!(
                &req.messages[message].parts[index],
                Part::ToolResult { content, .. } if content.len() > OMITTED_TOOL_RESULT.len()
            );
            if !replaceable {
                continue;
            }
            if let Part::ToolResult { content, .. } = &mut req.messages[message].parts[index] {
                *content = OMITTED_TOOL_RESULT.to_owned();
            }
            if prompt_size(&req) <= cap {
                break 'old;
            }
        }
    }
    // Step B: the newest message's tool results are cut largest first, never below the
    // floor, until the prompt fits.
    if prompt_size(&req) > cap
        && let Some(last) = req.messages.len().checked_sub(1)
    {
        let mut targets: Vec<usize> = req.messages[last]
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| matches!(part, Part::ToolResult { .. }))
            .map(|(index, _)| index)
            .collect();
        targets.sort_by_key(|index| match &req.messages[last].parts[*index] {
            Part::ToolResult { content, .. } => std::cmp::Reverse(content.len()),
            _ => std::cmp::Reverse(0),
        });
        for index in targets {
            let over = prompt_size(&req).saturating_sub(cap);
            if over == 0 {
                break;
            }
            if let Part::ToolResult { content, .. } = &mut req.messages[last].parts[index] {
                let shrinkable = content.len().saturating_sub(MIN_TOOL_RESULT_BYTES);
                let take = shrinkable.min(over);
                if take > 0 {
                    let keep = content.len() - take;
                    let cut = tools::cut(content, keep);
                    *content = cut;
                }
            }
        }
    }
    if prompt_size(&req) > cap {
        return Err(LlmError::Unavailable(format!(
            "the prompt is larger than the configured limit of {cap} bytes"
        )));
    }
    Ok((req, true))
}

#[cfg(test)]
mod tests {
    use super::{fit_to_cap, prompt_size, redact_error};
    use domovoi_core::ports::{LlmCapability, LlmError, LlmMessage, LlmRequest, Part, Role};
    use domovoi_testkit::ListRedactor;

    fn request(system: &str, messages: Vec<LlmMessage>) -> LlmRequest {
        LlmRequest {
            system: system.to_owned(),
            messages,
            tools: Vec::new(),
            max_output_tokens: 100,
        }
    }

    fn text(role: Role, text: &str) -> LlmMessage {
        LlmMessage {
            role,
            parts: vec![Part::Text(text.to_owned())],
        }
    }

    fn result(id: &str, content: &str) -> LlmMessage {
        LlmMessage {
            role: Role::User,
            parts: vec![Part::ToolResult {
                id: id.to_owned(),
                content: content.to_owned(),
                is_error: false,
            }],
        }
    }

    #[test]
    fn a_prompt_under_the_cap_is_unchanged() {
        let req = request("system", vec![text(Role::User, "opening")]);
        let (fitted, trimmed) = fit_to_cap(req.clone(), 1000).unwrap();
        assert_eq!(fitted, req);
        assert!(!trimmed);
    }

    #[test]
    fn old_tool_results_become_a_placeholder_first() {
        let opening = "o".repeat(500);
        let req = request(
            "",
            vec![
                text(Role::User, &opening),
                result("a", &"a".repeat(3000)),
                result("b", &"b".repeat(3000)),
                text(Role::User, "newest"),
            ],
        );
        let (fitted, trimmed) = fit_to_cap(req, 1000).unwrap();
        assert!(trimmed);
        assert!(prompt_size(&fitted) <= 1000);
        assert_eq!(
            fitted.messages[0].parts,
            vec![Part::Text(opening)],
            "the opening message is never trimmed"
        );
        for message in &fitted.messages[1..3] {
            assert!(matches!(
                &message.parts[0],
                Part::ToolResult { content, .. }
                    if content.contains("earlier tool output omitted")
            ));
        }
        assert_eq!(
            fitted.messages[3].parts,
            vec![Part::Text("newest".to_owned())]
        );
    }

    #[test]
    fn the_newest_tool_result_is_cut_no_below_the_floor() {
        let req = request(
            "",
            vec![
                text(Role::User, &"o".repeat(1000)),
                result("newest", &"n".repeat(5000)),
            ],
        );
        let (fitted, trimmed) = fit_to_cap(req, 3000).unwrap();
        assert!(trimmed);
        assert!(prompt_size(&fitted) <= 3000);
        let Part::ToolResult { content, .. } = &fitted.messages[1].parts[0] else {
            unreachable!("the newest tool result stays a tool result");
        };
        assert!(
            content.len() >= 1024,
            "the floor is kept: {}",
            content.len()
        );
        assert!(content.ends_with("[output cut]") || content.len() < 5000);
    }

    #[test]
    fn a_prompt_that_cannot_fit_is_an_unavailable_error() {
        let req = request("", vec![text(Role::User, &"o".repeat(5000))]);
        assert_eq!(
            fit_to_cap(req, 1000),
            Err(LlmError::Unavailable(
                "the prompt is larger than the configured limit of 1000 bytes".to_owned()
            ))
        );
    }

    #[test]
    fn redact_error_removes_a_secret_from_every_text_variant() {
        let redactor = ListRedactor::new(["s3cret"]);
        let secret = "s3cret";
        assert_eq!(
            redact_error(&redactor, LlmError::Transport(format!("sent {secret}"))),
            LlmError::Transport("sent [REDACTED]".to_owned())
        );
        assert_eq!(
            redact_error(
                &redactor,
                LlmError::InvalidResponse(format!("body {secret}"))
            ),
            LlmError::InvalidResponse("body [REDACTED]".to_owned())
        );
        assert_eq!(
            redact_error(&redactor, LlmError::Unavailable(format!("down {secret}"))),
            LlmError::Unavailable("down [REDACTED]".to_owned())
        );
        assert_eq!(
            redact_error(
                &redactor,
                LlmError::CapabilityRejected {
                    capability: LlmCapability::Tools,
                    detail: format!("refused {secret}"),
                }
            ),
            LlmError::CapabilityRejected {
                capability: LlmCapability::Tools,
                detail: "refused [REDACTED]".to_owned(),
            }
        );
        assert_eq!(
            redact_error(&redactor, LlmError::Auth),
            LlmError::Auth,
            "a variant without text is unchanged"
        );
    }
}
