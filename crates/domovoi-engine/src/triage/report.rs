//! Reading the model's final answer as a [`TriageReport`].

use domovoi_core::TriageReport;

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

fn strip_fences(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop the info string (`json`) on the opening fence's line.
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}
