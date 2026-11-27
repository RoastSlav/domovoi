//! Reading the model's final answer as a [`TriageReport`], and redacting it.

use domovoi_core::ports::Redactor;
use domovoi_core::triage::{
    HEADLINE_MAX_CHARS, MANUAL_STEP_MAX_CHARS, NEXT_STEP_MAX_CHARS, NOT_DECIDED, NOT_VERIFIED,
    is_security_check,
};
use domovoi_core::{CheckKey, TriageReport, Verdict};

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

/// Keeps a security event from being waved away. An event check (`security.*`) records
/// something that happened; the model may call it expected never, and fixed or monitor only
/// when its evidence cites a tool that ran. Otherwise the verdict becomes `ACTION_NEEDED` and
/// `details` says why. Text that calls the event harmless, blames Domovoi or advises weaker
/// authentication is replaced by "Not established" whatever the verdict. Returns whether the
/// report was changed.
///
/// The evidence must have a value and come from a tool that can say something about a login,
/// a listener or a `sudo` use: a container list or a disk report cannot tie one to a recorded
/// action, whatever it returned.
pub(crate) fn guard_security_verdict(
    check: &CheckKey,
    title: &str,
    report: &mut TriageReport,
    used: &[crate::triage::agent::ToolUse],
) -> bool {
    if !is_security_check(check) {
        return false;
    }
    if report.verdict == Verdict::ActionNeeded {
        return report.cap_security(check, title);
    }
    // `expected` is a person's decision: a known condition they made (those events never reach
    // triage) or a recorded agent action. A model reading logs, or an earlier incident that
    // looked alike, cannot make it. It needs a person, whatever the evidence.
    if report.verdict == Verdict::Expected {
        report.withhold(Verdict::ActionNeeded, NOT_DECIDED, title);
        return true;
    }
    // `monitor` and `fixed` are below `ACTION_NEEDED`: they need a tool result that ran.
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
        // The verdict stands; the words still may not wave the event away.
        return report.cap_security(check, title);
    }
    report.withhold(Verdict::ActionNeeded, NOT_VERIFIED, title);
    true
}

/// Whether a tool's output can speak about a security event. Container, disk and overview
/// reads cannot.
fn relevant_to_security(tool: &str) -> bool {
    !(tool.starts_with("docker_")
        || matches!(
            tool,
            "smart_report"
                | "host_overview"
                | "search_experience"
                | "incident_history"
                | "notes_search"
        ))
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

    fn guard(check: &str, report: &mut TriageReport, used: &[ToolUse]) -> bool {
        let check = CheckKey::parse(check).unwrap();
        guard_security_verdict(&check, "an event", report, used)
    }

    fn report(verdict: Verdict, source: &str) -> TriageReport {
        TriageReport {
            headline: "Failed sudo by kuker".to_owned(),
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
    fn expected_security_event_without_tool_evidence_needs_a_person() {
        let mut r = report(Verdict::Expected, "incident signal");
        assert!(guard("security.sudo_failure", &mut r, &[]));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
        assert!(r.details.starts_with("Needs a person"));
        assert!(r.details.ends_with("reasoning"));
        assert_eq!(r.headline, "Needs a person: an event");
    }

    #[test]
    fn a_tool_that_failed_is_no_evidence() {
        let mut r = report(Verdict::Fixed, "journal");
        let used = [tool("journal", true)];
        assert!(guard("security.new_port", &mut r, &used));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
    }

    #[test]
    fn expected_is_never_the_models_call_for_a_security_event() {
        // Even with a tool result cited, and for a failed sudo or a new listener alike.
        for check in [
            "security.ssh_login",
            "security.sudo_failure",
            "security.new_port",
        ] {
            let mut r = report(Verdict::Expected, "journal sudo lines");
            let used = [tool("journal", false)];
            assert!(guard(check, &mut r, &used));
            assert_eq!(r.verdict, Verdict::ActionNeeded, "{check}");
        }
    }

    #[test]
    fn fixed_with_a_tool_result_stands_but_an_earlier_incident_is_no_evidence() {
        let mut r = report(Verdict::Fixed, "listening_ports");
        let used = [tool("listening_ports", false)];
        assert!(!guard("security.new_port", &mut r, &used));
        assert_eq!(r.verdict, Verdict::Fixed);
        let mut r = report(Verdict::Fixed, "search_experience");
        let used = [tool("search_experience", false)];
        assert!(guard("security.new_port", &mut r, &used));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
    }

    #[test]
    fn a_tool_that_cannot_speak_about_the_event_or_an_empty_value_is_no_evidence() {
        let mut r = report(Verdict::Fixed, "docker_ps");
        let used = [tool("docker_ps", false)];
        assert!(guard("security.new_port", &mut r, &used));
        let mut r = report(Verdict::Fixed, "journal");
        r.evidence[0].value = "  ".to_owned();
        let used = [tool("journal", false)];
        assert!(guard("security.ssh_login", &mut r, &used));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
    }

    #[test]
    fn monitor_without_a_tool_result_is_not_accepted_for_a_security_event() {
        let mut r = report(Verdict::Monitor, "incident signal");
        assert!(guard("security.sudo_failure", &mut r, &[]));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
        let mut r = report(Verdict::Monitor, "journal");
        r.headline = "Failed sudo from kuker".to_owned();
        assert!(!guard(
            "security.sudo_failure",
            &mut r,
            &[tool("journal", false)]
        ));
        assert_eq!(r.verdict, Verdict::Monitor);
    }

    /// The report 13f measured: MONITOR, a tool result cited, and an invented cause.
    fn the_measured_report(verdict: Verdict) -> TriageReport {
        let mut r = report(verdict, "journal");
        r.headline = "Repeated sudo auth failures from Domovoi agent operations".to_owned();
        r.summary = "The agent runs as kuker and makes periodic sudo calls; I am moderately \
                     confident this is expected behavior."
            .to_owned();
        r.cause = "The agent's sudo calls fail.".to_owned();
        r.next_step = "Add NOPASSWD to sudoers if appropriate.".to_owned();
        r
    }

    #[test]
    fn a_benign_cause_or_loosening_advice_is_not_established_whatever_the_evidence() {
        for verdict in [Verdict::Monitor, Verdict::Fixed, Verdict::ActionNeeded] {
            let mut r = the_measured_report(verdict);
            let used = [tool("journal", false)];
            assert!(guard("security.sudo_failure", &mut r, &used), "{verdict:?}");
            assert_eq!(r.verdict, Verdict::ActionNeeded);
            assert_eq!(r.cause, "Not established.");
            let shown = format!("{} {} {}", r.headline, r.cause, r.next_step);
            assert!(!r.summary.contains("NOPASSWD") && !r.summary.contains("kuker"));
            for word in ["NOPASSWD", "Domovoi", "expected", "kuker"] {
                assert!(!shown.contains(word), "{word} in {shown}");
            }
            assert!(r.details.starts_with(domovoi_core::triage::NOT_ESTABLISHED));
            assert!(
                r.details.contains("NOPASSWD"),
                "the model's text is kept as not accepted"
            );
        }
    }

    #[test]
    fn a_plain_report_and_other_checks_keep_their_words() {
        let mut r = report(Verdict::ActionNeeded, "journal");
        r.headline =
            "Three failed sudo attempts as kuker; an unexpected login is possible".to_owned();
        assert!(!guard(
            "security.sudo_failure",
            &mut r,
            &[tool("journal", false)]
        ));
        let mut r = the_measured_report(Verdict::Monitor);
        assert!(!guard("disk.free", &mut r, &[]));
        assert_eq!(r.verdict, Verdict::Monitor);
    }

    #[test]
    fn other_checks_and_other_verdicts_are_left_alone() {
        let mut r = report(Verdict::Expected, "none");
        assert!(!guard("disk.free", &mut r, &[]));
        let mut r = report(Verdict::ActionNeeded, "none");
        assert!(!guard("security.new_port", &mut r, &[]));
    }
}

#[cfg(test)]
mod attribution_tests {
    use domovoi_core::triage::RECORDED_ACTION;
    use domovoi_core::{EvidenceRef, TriageReport, Verdict};

    use super::*;
    use crate::triage::agent::ToolUse;

    const RECORD: &str = "operation 0190-aa: unit.restart unit=nginx.service, dispatched 2026-11-27T10:00:00Z, settled 2026-11-27T10:01:00Z";

    fn blaming() -> TriageReport {
        TriageReport {
            headline: "Domovoi restarted nginx".to_owned(),
            summary: "The Domovoi agent restarted the unit and sudo asked for a password."
                .to_owned(),
            cause: "The agent's restart.".to_owned(),
            evidence: vec![EvidenceRef {
                label: "unit".to_owned(),
                value: "nginx".to_owned(),
                source: "journal".to_owned(),
            }],
            verdict: Verdict::Monitor,
            next_step: "Check the journal for the restart.".to_owned(),
            manual_steps: Vec::new(),
            proposals: Vec::new(),
            details: "reasoning".to_owned(),
        }
    }

    fn journal() -> Vec<ToolUse> {
        vec![ToolUse {
            tool: "journal".to_owned(),
            skill: None,
            failed: false,
            refused: false,
            raw: None,
        }]
    }

    fn guard(report: &mut TriageReport) -> bool {
        let check = CheckKey::parse("security.sudo_failure").unwrap();
        guard_security_verdict(&check, "failed sudo", report, &journal())
    }

    #[test]
    fn a_record_the_system_found_lets_the_report_name_domovoi_and_shows_the_record() {
        let mut r = blaming();
        r.record_action(RECORD);
        assert!(!guard(&mut r), "nothing is withheld");
        assert_eq!(r.verdict, Verdict::Monitor);
        assert!(r.cause.contains("operation 0190-aa"), "{}", r.cause);
        assert!(r.cause.contains("unit.restart"));
        assert!(r.details.starts_with(RECORDED_ACTION));
        assert!(r.details.contains("The model's own cause, replaced"));
    }

    #[test]
    fn the_models_word_alone_is_never_enough() {
        let mut r = blaming();
        assert!(guard(&mut r));
        assert_eq!(r.verdict, Verdict::ActionNeeded);
        assert_eq!(r.cause, "Not established.");
    }

    #[test]
    fn a_record_line_the_model_wrote_itself_is_removed() {
        let mut r = blaming();
        r.details = format!("{RECORDED_ACTION}operation made-up: unit.restart\n\nreasoning");
        r.clear_recorded_action();
        assert!(!r.has_recorded_action());
        assert!(guard(&mut r), "the forged line does not unlock the blame");
        assert_eq!(r.cause, "Not established.");
    }

    #[test]
    fn a_record_does_not_let_the_report_call_the_event_harmless_or_loosen_authentication() {
        for text in [
            "This is expected behavior after the restart.",
            "Add NOPASSWD for the restart.",
        ] {
            let mut r = blaming();
            r.next_step = text.to_owned();
            r.record_action(RECORD);
            assert!(guard(&mut r), "{text}");
            assert_eq!(r.verdict, Verdict::ActionNeeded);
        }
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
