//! Folding the `triage.tools` timeline entries into the counts the incident page shows.

use domovoi_core::ports::IncidentTimelineEntry;
use serde::Deserialize;

use super::dto::ToolUsageDto;

/// The `tools` in a `triage.tools` timeline entry.
#[derive(Deserialize)]
struct ToolUseEntry {
    tool: String,
    skill: Option<String>,
    failed: bool,
}

/// Folds the `triage.tools` entries in `timeline` into one row per tool or skill, in
/// first-call order. A read skill is listed under its skill name, not `run_read_skill`.
pub(crate) fn tool_usage(timeline: &[IncidentTimelineEntry]) -> Vec<ToolUsageDto> {
    let mut rows: Vec<ToolUsageDto> = Vec::new();
    for entry in timeline.iter().filter(|e| e.kind == "triage.tools") {
        let Ok(tools) = serde_json::from_value::<Vec<ToolUseEntry>>(entry.detail["tools"].clone())
        else {
            continue;
        };
        for tool in tools {
            let (name, kind) = match tool.skill {
                Some(skill) => (skill, "skill"),
                None => (tool.tool, "tool"),
            };
            match rows.iter_mut().find(|row| row.name == name) {
                Some(row) => {
                    row.calls += 1;
                    if tool.failed {
                        row.failures += 1;
                    }
                    row.last_used = entry.at.to_string();
                }
                None => rows.push(ToolUsageDto {
                    name,
                    kind: kind.to_owned(),
                    calls: 1,
                    failures: u32::from(tool.failed),
                    last_used: entry.at.to_string(),
                }),
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use domovoi_core::TriageOutcome;

    #[test]
    fn tool_usage_folds_entries_in_first_call_order() {
        let incident = domovoi_core::IncidentId::new();
        let entry = |at: &str, tools: serde_json::Value| IncidentTimelineEntry {
            incident,
            at: at.parse().unwrap(),
            kind: "triage.tools".to_owned(),
            summary: String::new(),
            detail: serde_json::json!({ "during": "triage", "tools": tools }),
        };
        let timeline = vec![
            entry(
                "2026-01-01T00:00:00Z",
                serde_json::json!([
                    {"tool": "journal", "skill": null, "failed": false},
                    {"tool": "run_read_skill", "skill": "logs", "failed": true},
                ]),
            ),
            entry(
                "2026-01-01T00:01:00Z",
                serde_json::json!([{"tool": "journal", "skill": null, "failed": true}]),
            ),
        ];

        let rows = tool_usage(&timeline);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "journal");
        assert_eq!(rows[0].kind, "tool");
        assert_eq!(rows[0].calls, 2);
        assert_eq!(rows[0].failures, 1);
        assert_eq!(rows[0].last_used, "2026-01-01T00:01:00Z");
        assert_eq!(rows[1].name, "logs");
        assert_eq!(rows[1].kind, "skill");
        assert_eq!(rows[1].failures, 1);
    }

    #[test]
    fn latest_triage_ignores_tool_use_entries() {
        let incident = domovoi_core::IncidentId::new();
        let entry = |kind: &str, detail: serde_json::Value| IncidentTimelineEntry {
            incident,
            at: "2026-01-01T00:00:00Z".parse().unwrap(),
            kind: kind.to_owned(),
            summary: String::new(),
            detail,
        };
        let timeline = vec![
            entry(
                "triage.skipped",
                serde_json::json!({"type": "skipped", "data": {"reason": "budget"}}),
            ),
            entry(
                "triage.tools",
                serde_json::json!({"during": "triage", "tools": []}),
            ),
        ];
        assert_eq!(
            super::super::incidents::latest_triage(&timeline),
            Some(TriageOutcome::Skipped {
                reason: "budget".to_owned()
            })
        );
    }
}
