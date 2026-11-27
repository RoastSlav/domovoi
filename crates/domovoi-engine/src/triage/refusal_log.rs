//! The log line that says what the security guard refused, so a person reading the log can
//! tell whether the guard was right.

use domovoi_core::triage::{NOT_DECIDED, NOT_VERIFIED};
use domovoi_core::{IncidentId, TriageReport, Verdict};

/// The name of the rule that refused `report`, read from how its `details` open.
#[must_use]
pub(crate) fn refusal_rule(report: &TriageReport) -> &'static str {
    if report.details.starts_with(NOT_DECIDED) {
        "expected_is_a_persons_decision"
    } else if report.details.starts_with(NOT_VERIFIED) {
        "verdict_needs_a_tool_result"
    } else {
        "claims_without_evidence"
    }
}

/// Logs that the guard replaced a model's verdict or wording on `incident`: what verdict the
/// model gave, which rule refused it, why, and what the verdict is now. `report` is the
/// report after the guard ran.
pub(crate) fn log_refusal(incident: IncidentId, model_verdict: Verdict, report: &TriageReport) {
    let reason = report.details.split("\n\n").next().unwrap_or("").trim();
    tracing::warn!(
        %incident,
        model_verdict = %model_verdict.label(),
        rule = %refusal_rule(report),
        reason,
        verdict_now = report.verdict.label(),
        "a model's security verdict or wording was refused"
    );
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::log_capture::LogCapture;

    use super::*;

    fn refused(verdict: Verdict, details: &str) -> TriageReport {
        TriageReport {
            headline: "Needs a person: an event".to_owned(),
            summary: String::new(),
            cause: String::new(),
            evidence: Vec::new(),
            verdict,
            next_step: String::new(),
            manual_steps: Vec::new(),
            proposals: Vec::new(),
            details: details.to_owned(),
        }
    }

    #[test]
    fn the_line_says_what_the_model_said_and_which_rule_refused_it() {
        let (logs, _guard) = LogCapture::start();
        let report = refused(
            Verdict::ActionNeeded,
            &format!("{NOT_DECIDED}\n\nThe model's own text, not accepted:"),
        );
        log_refusal(IncidentId::new(), Verdict::Expected, &report);
        let line = logs.lines_with("was refused").remove(0);
        for field in [
            "WARN",
            "model_verdict=EXPECTED",
            "rule=expected_is_a_persons_decision",
            "verdict_now=\"ACTION NEEDED\"",
            "reason=\"Needs a person",
        ] {
            assert!(line.contains(field), "lacks {field}: {line}");
        }
    }

    #[test]
    fn the_rule_follows_the_reason() {
        let verified = refused(Verdict::ActionNeeded, NOT_VERIFIED);
        assert_eq!(refusal_rule(&verified), "verdict_needs_a_tool_result");
        let other = refused(Verdict::ActionNeeded, "something else");
        assert_eq!(refusal_rule(&other), "claims_without_evidence");
    }
}
