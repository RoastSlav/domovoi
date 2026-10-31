use serde_json::json;

use super::{ActionProposal, ReadTool, ReadToolCall, ToolArgError, TriageReport};
use crate::skill::SkillName;
use crate::verdict::Verdict;

fn report() -> TriageReport {
    TriageReport {
        headline: "plex is crash-looping after an image update".to_owned(),
        summary: "The container restarts every 40 seconds.".to_owned(),
        cause: "The new image needs a config migration.".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::ActionNeeded,
        next_step: "Roll back to the previous image tag.".to_owned(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

fn proposal() -> ActionProposal {
    ActionProposal {
        skill: SkillName::parse("restart_container").unwrap(),
        args: Default::default(),
        reason: "restart".to_owned(),
    }
}

#[test]
fn triage_report_validation_rejects_long_headline_and_too_many_proposals() {
    assert!(report().validate().is_ok());

    let mut long = report();
    long.headline = "x".repeat(121);
    assert_eq!(long.validate().unwrap_err().field, "headline");

    let mut empty = report();
    empty.headline = String::new();
    assert!(empty.validate().is_err());

    let mut many = report();
    many.proposals = vec![proposal(), proposal(), proposal(), proposal()];
    assert_eq!(many.validate().unwrap_err().field, "proposals");

    let mut long_step = report();
    long_step.next_step = "x".repeat(301);
    assert_eq!(long_step.validate().unwrap_err().field, "next_step");
}

#[test]
fn triage_report_rejects_unknown_fields() {
    let mut value = serde_json::to_value(report()).unwrap();
    assert!(serde_json::from_value::<TriageReport>(value.clone()).is_ok());
    value["approve"] = json!(true);
    assert!(serde_json::from_value::<TriageReport>(value).is_err());

    let nested = json!({
        "headline": "h", "summary": "s", "cause": "c", "verdict": "MONITOR", "next_step": "n",
        "proposals": [{ "skill": "restart_container", "reason": "r", "command": "rm -rf /" }],
    });
    assert!(serde_json::from_value::<TriageReport>(nested).is_err());
}

#[test]
fn tool_names_round_trip() {
    for tool in ReadTool::ALL {
        assert_eq!(ReadTool::from_name(tool.name()), Some(tool));
    }
}

#[test]
fn tool_calls_apply_defaults_and_bounds() {
    let call = ReadToolCall::from_json("docker_logs", json!({ "container": "plex" })).unwrap();
    assert_eq!(
        call,
        ReadToolCall::DockerLogs {
            container: "plex".to_owned(),
            lines: 100
        }
    );
    let bad = [
        ("docker_logs", json!({ "container": "plex", "lines": 501 })),
        ("docker_logs", json!({ "container": "plex", "lines": 0 })),
        ("docker_logs", json!({ "container": "" })),
        ("docker_logs", json!({ "container": "pl\nex" })),
        (
            "docker_logs",
            json!({ "container": "plex", "follow": true }),
        ),
        ("journal", json!({ "since_minutes": 1441 })),
        ("smart_report", json!({ "device": "/etc/passwd" })),
        ("smart_report", json!({ "device": "/dev/../etc/passwd" })),
        ("notes_search", json!({ "query": "x".repeat(201) })),
        ("incident_history", json!({ "limit": 51 })),
        ("host_overview", json!({ "verbose": true })),
        ("run_read_skill", json!({ "skill": "Bad Name" })),
    ];
    for (tool, args) in bad {
        let result = ReadToolCall::from_json(tool, args.clone());
        assert!(
            matches!(result, Err(ToolArgError::InvalidArgs { .. })),
            "{tool} {args} was accepted"
        );
    }
}

#[test]
fn every_tool_accepts_its_minimal_arguments() {
    let minimal = |tool: ReadTool| match tool {
        ReadTool::DockerLogs | ReadTool::DockerInspect => json!({ "container": "plex" }),
        ReadTool::SmartReport => json!({ "device": "/dev/sda" }),
        ReadTool::NotesSearch => json!({ "query": "zfs" }),
        ReadTool::RunReadSkill => json!({ "skill": "zpool_status" }),
        _ => json!(null),
    };
    for tool in ReadTool::ALL {
        let call = ReadToolCall::from_json(tool.name(), minimal(tool)).unwrap();
        assert_eq!(call.tool(), tool);
        let required = tool.json_schema()["required"].as_array().unwrap().len();
        let given = minimal(tool).as_object().map_or(0, |o| o.len());
        assert_eq!(
            required,
            given,
            "{} schema and parser disagree",
            tool.name()
        );
    }
}
