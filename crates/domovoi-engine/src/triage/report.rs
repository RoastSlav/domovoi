//! Reading the model's final answer as a [`TriageReport`], and redacting it.

use domovoi_core::ports::Redactor;
use domovoi_core::triage::{HEADLINE_MAX_CHARS, MANUAL_STEP_MAX_CHARS, NEXT_STEP_MAX_CHARS};
use domovoi_core::{TriageReport, Verdict};

use crate::cards::clip;

/// Reads `text` as one JSON report and validates it. Code fences and prose around the
/// object are ignored. The error says what to fix, for the repair turn.
pub fn parse_report(text: &str) -> Result<TriageReport, String> {
    let text = strip_fences(text.trim());
    let json = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if start < end => &text[start..=end],
        _ => return Err("the answer contains no JSON object".to_owned()),
    };
    let report: TriageReport = serde_json::from_str(json)
        .map_err(|e| format!("the JSON does not match the schema: {e}"))?;
    report
        .validate()
        .map_err(|e| format!("the report is not valid: {e}"))?;
    Ok(report)
}

/// Redacts every free-text field of `report` (INV-5), keeping the length limits. By-hand
/// steps that are empty after redaction are dropped, so the checklist never renders a blank
/// item. Proposal arguments are left alone: they are validated against the skill's
/// parameters, and changing them would change what runs.
pub(crate) fn redact_report(redactor: &dyn Redactor, report: &mut TriageReport) {
    report.headline = clip(&redactor.redact(&report.headline), HEADLINE_MAX_CHARS);
    report.summary = redactor.redact(&report.summary);
    report.cause = redactor.redact(&report.cause);
    for evidence in &mut report.evidence {
        evidence.label = redactor.redact(&evidence.label);
        evidence.value = redactor.redact(&evidence.value);
        evidence.source = redactor.redact(&evidence.source);
    }
    report.next_step = clip(&redactor.redact(&report.next_step), NEXT_STEP_MAX_CHARS);
    report.manual_steps = report
        .manual_steps
        .iter()
        .map(|step| clip(&redactor.redact(step), MANUAL_STEP_MAX_CHARS))
        .filter(|step| !step.trim().is_empty())
        .collect();
    for proposal in &mut report.proposals {
        proposal.reason = redactor.redact(&proposal.reason);
    }
    report.details = redactor.redact(&report.details);
}

/// The sentence that opens `details` when the guard below changed a verdict.
pub(crate) const NOT_VERIFIED: &str = "Not verified: the model called this event expected, but \
no tool result in its report ties it to a recorded action, so it needs a person.";

/// Keeps a security event from being waved away. An event check (`security.*`) records
/// something that happened; the model may call it expected or fixed only when its evidence
/// cites a tool that ran. Otherwise the verdict becomes `MONITOR` and `details` says why.
/// Returns whether the verdict was changed.
///
/// The evidence must have a value and come from a tool that can say something about a login,
/// a listener or a `sudo` use: a container list or a disk report cannot tie one to a recorded
/// action, whatever it returned.
pub(crate) fn guard_security_verdict(
    check: &str,
    report: &mut TriageReport,
    used: &[crate::triage::agent::ToolUse],
) -> bool {
    if !check.starts_with("security.")
        || !matches!(report.verdict, Verdict::Expected | Verdict::Fixed)
    {
        return false;
    }
    let tied = report.evidence.iter().any(|item| {
        !item.value.trim().is_empty()
            && used.iter().any(|tool| {
                !tool.failed
                    && !tool.refused
                    && relevant_to_security(&tool.tool)
                    && item.source.contains(&tool.tool)
            })
    });
    if tied {
        return false;
    }
    report.verdict = Verdict::Monitor;
    report.details = if report.details.trim().is_empty() {
        NOT_VERIFIED.to_owned()
    } else {
        format!("{NOT_VERIFIED}\n\n{}", report.details)
    };
    true
}

/// Whether a tool's output can speak about a security event. Container, disk and overview
/// reads cannot.
fn relevant_to_security(tool: &str) -> bool {
    !(tool.starts_with("docker_") || matches!(tool, "smart_report" | "host_overview"))
}

fn strip_fences(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop the info string (`json`) on the opening fence's line.
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

#[cfg(test)]
mod guard_tests {
    use domovoi_core::{TriageReport, Verdict};

    use super::*;
    use crate::triage::agent::ToolUse;

    fn report(verdict: Verdict, source: &str) -> TriageReport {
        TriageReport {
            headline: "Failed sudo likely a self-test".to_owned(),
            summary: String::new(),
            cause: String::new(),
            evidence: vec![domovoi_core::triage::EvidenceRef {
                label: "user".to_owned(),
                value: "kuker".to_owned(),
                source: source.to_owned(),
            }],
            verdict,
            next_step: String::new(),
            manual_steps: Vec::new(),
            proposals: Vec::new(),
            details: "reasoning".to_owned(),
        }
    }

    fn tool(name: &str, failed: bool) -> ToolUse {
        ToolUse {
            tool: name.to_owned(),
            skill: None,
            failed,
            refused: false,
            raw: None,
        }
    }

    #[test]
    fn expected_security_event_without_tool_evidence_becomes_monitor() {
        let mut r = report(Verdict::Expected, "incident signal");
        assert!(guard_security_verdict("security.sudo_failure", &mut r, &[]));
        assert_eq!(r.verdict, Verdict::Monitor);
        assert!(r.details.starts_with("Not verified"));
        assert!(r.details.ends_with("reasoning"));
    }

    #[test]
    fn a_tool_that_failed_is_no_evidence() {
        let mut r = report(Verdict::Fixed, "journal");
        let used = [tool("journal", true)];
        assert!(guard_security_verdict("security.new_port", &mut r, &used));
        assert_eq!(r.verdict, Verdict::Monitor);
    }

    #[test]
    fn expected_with_a_tool_result_stands() {
        let mut r = report(Verdict::Expected, "journal sudo lines");
        let used = [tool("journal", false)];
        assert!(!guard_security_verdict("security.ssh_login", &mut r, &used));
        assert_eq!(r.verdict, Verdict::Expected);
    }

    #[test]
    fn a_tool_that_cannot_speak_about_the_event_or_an_empty_value_is_no_evidence() {
        let mut r = report(Verdict::Expected, "docker_ps");
        let used = [tool("docker_ps", false)];
        assert!(guard_security_verdict("security.new_port", &mut r, &used));
        let mut r = report(Verdict::Expected, "journal");
        r.evidence[0].value = "  ".to_owned();
        let used = [tool("journal", false)];
        assert!(guard_security_verdict("security.ssh_login", &mut r, &used));
        assert_eq!(r.verdict, Verdict::Monitor);
    }

    #[test]
    fn other_checks_and_other_verdicts_are_left_alone() {
        let mut r = report(Verdict::Expected, "none");
        assert!(!guard_security_verdict("disk.free", &mut r, &[]));
        let mut r = report(Verdict::ActionNeeded, "none");
        assert!(!guard_security_verdict("security.new_port", &mut r, &[]));
    }
}

#[cfg(test)]
mod prompt_tests {
    #[test]
    fn both_prompts_tell_the_model_a_security_event_cannot_be_disproved_by_now() {
        for prompt in [
            crate::triage::SYSTEM_PROMPT,
            crate::triage::meta::META_SYSTEM_PROMPT,
        ] {
            assert!(prompt.contains("## Security checks"));
            assert!(prompt.contains("can end the event, never disprove it"));
        }
    }

    #[test]
    fn the_prompt_says_ask_only_what_no_tool_can_tell() {
        assert!(
            crate::triage::meta::META_SYSTEM_PROMPT.contains("Ask only what no tool can tell you")
        );
    }
}
