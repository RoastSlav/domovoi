//! The report's "Proposals not offered" lines. A proposal is only data: the plan validator
//! decides which ones become steps, and nothing else in the report is ever acted on (INV-9).

use std::fmt::Write as _;

use domovoi_core::TriageReport;

/// Appends a "Proposals not offered:" list of `dropped` lines to the report's details, so
/// the reader learns why a proposal has no action. Nothing is added for an empty list.
pub(crate) fn append_dropped(report: &mut TriageReport, dropped: Vec<String>) {
    if dropped.is_empty() {
        return;
    }
    let mut details = std::mem::take(&mut report.details);
    if !details.trim().is_empty() {
        details.push_str("\n\n");
    }
    details.push_str("Proposals not offered:");
    for line in dropped {
        let _ = write!(details, "\n- {line}");
    }
    report.details = details;
}

#[cfg(test)]
mod append_tests {
    use domovoi_core::TriageReport;

    use super::append_dropped;

    fn report(details: &str) -> TriageReport {
        let mut report: TriageReport = serde_json::from_value(serde_json::json!({
            "headline": "h", "summary": "s", "cause": "c", "verdict": "MONITOR",
            "next_step": "n",
        }))
        .expect("a minimal report parses");
        report.details = details.to_owned();
        report
    }

    #[test]
    fn append_dropped_formats_the_not_offered_lines() {
        let mut with_details = report("Why.");
        append_dropped(
            &mut with_details,
            vec!["a: not a skill on this host".to_owned()],
        );
        assert_eq!(
            with_details.details,
            "Why.\n\nProposals not offered:\n- a: not a skill on this host"
        );
        let mut empty = report("");
        append_dropped(&mut empty, vec!["a: x".to_owned(), "b: y".to_owned()]);
        assert_eq!(empty.details, "Proposals not offered:\n- a: x\n- b: y");
        let mut none = report("Why.");
        append_dropped(&mut none, Vec::new());
        assert_eq!(none.details, "Why.");
    }
}
