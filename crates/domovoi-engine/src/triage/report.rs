//! Reading the model's final answer as a [`TriageReport`], and redacting it.

use domovoi_core::TriageReport;
use domovoi_core::ports::Redactor;
use domovoi_core::triage::{HEADLINE_MAX_CHARS, NEXT_STEP_MAX_CHARS};

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

/// Redacts every free-text field of `report` (INV-5), keeping the length limits. Proposal
/// arguments are left alone: they are validated against the skill's parameters, and
/// changing them would change what runs.
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
    for proposal in &mut report.proposals {
        proposal.reason = redactor.redact(&proposal.reason);
    }
    report.details = redactor.redact(&report.details);
}

fn strip_fences(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop the info string (`json`) on the opening fence's line.
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}
