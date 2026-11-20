use serde_json::json;

use super::*;
use crate::triage::{MAX_PROPOSALS, ReadTool, ReadToolCall};

fn report() -> serde_json::Value {
    json!({
        "headline": "Plex restarts",
        "summary": "s",
        "cause": "c",
        "verdict": "MONITOR",
        "next_step": "wait",
    })
}

#[test]
fn meta_tool_names_and_order_are_pinned() {
    let names: Vec<&str> = MetaTool::ALL.iter().map(MetaTool::name).collect();
    assert_eq!(
        names,
        [
            "list_capabilities",
            "describe_capability",
            "request_evidence",
            "propose_plan",
            "ask_user",
            "finish"
        ]
    );
    for tool in MetaTool::ALL {
        assert_eq!(MetaTool::from_name(tool.name()), Some(tool));
    }
    assert_eq!(MetaTool::from_name("approve_action"), None);
}

#[test]
fn read_set_is_the_first_three_and_investigate_depends_on_questions() {
    assert_eq!(ToolSet::Read.tools(), MetaTool::READ.to_vec());
    assert_eq!(ToolSet::Investigate { questions: false }.tools().len(), 5);
    assert!(
        !ToolSet::Investigate { questions: false }
            .tools()
            .contains(&MetaTool::AskUser)
    );
    assert_eq!(
        ToolSet::Investigate { questions: true }.tools(),
        MetaTool::ALL.to_vec()
    );
}

#[test]
fn every_meta_tool_schema_is_a_closed_object() {
    for tool in MetaTool::ALL {
        let schema = tool.json_schema();
        assert_eq!(schema["type"], "object", "{}", tool.name());
        assert_eq!(schema["additionalProperties"], false, "{}", tool.name());
    }
    let tool_enum = MetaTool::RequestEvidence.json_schema()["properties"]["tool"]["enum"].clone();
    let expected: Vec<&str> = ReadTool::ALL.iter().map(ReadTool::name).collect();
    assert_eq!(tool_enum, json!(expected));
    let finish = MetaTool::Finish.json_schema();
    assert!(finish["properties"].get("proposals").is_none());
    assert_eq!(
        MetaTool::ProposePlan.json_schema()["properties"]["steps"]["maxItems"],
        MAX_PROPOSALS
    );
}

#[test]
fn from_json_parses_each_tool() {
    assert!(matches!(
        MetaToolCall::from_json("list_capabilities", json!({})),
        Ok(MetaToolCall::ListCapabilities {
            effect: None,
            include_unavailable: false
        })
    ));
    assert!(matches!(
        MetaToolCall::from_json("describe_capability", json!({"contract": "skill.restart"})),
        Ok(MetaToolCall::DescribeCapability { .. })
    ));
    let call = MetaToolCall::from_json(
        "request_evidence",
        json!({"tool": "docker_logs", "args": {"container": "plex"}}),
    )
    .unwrap();
    assert_eq!(
        call,
        MetaToolCall::RequestEvidence(ReadToolCall::DockerLogs {
            container: "plex".to_owned(),
            lines: 100
        })
    );
    let plan = MetaToolCall::from_json(
        "propose_plan",
        json!({"steps": [{"contract": "skill.restart_container", "reason": "r"}]}),
    )
    .unwrap();
    assert_eq!(plan.tool(), MetaTool::ProposePlan);
    assert!(matches!(
        MetaToolCall::from_json("ask_user", json!({"question": "q?"})),
        Ok(MetaToolCall::AskUser { .. })
    ));
    assert!(matches!(
        MetaToolCall::from_json("finish", report()),
        Ok(MetaToolCall::Finish(_))
    ));
}

#[test]
fn finish_rejects_a_proposals_key_and_an_invalid_report() {
    let mut input = report();
    input["proposals"] = json!([]);
    let error = MetaToolCall::from_json("finish", input).unwrap_err();
    assert!(error.to_string().contains("use propose_plan"));
    let mut input = report();
    input["headline"] = json!("x".repeat(121));
    let error = MetaToolCall::from_json("finish", input).unwrap_err();
    assert!(error.to_string().contains("headline"));
}

#[test]
fn unknown_tool_text_keeps_the_words_unknown_tool() {
    let error = MetaToolCall::from_json("approve_action", json!({})).unwrap_err();
    assert_eq!(error.to_string(), "unknown tool `approve_action`");
    let old = MetaToolCall::from_json("docker_ps", json!({})).unwrap_err();
    assert!(matches!(old, MetaArgError::UnknownTool(_)));
}

#[test]
fn request_evidence_reuses_the_read_tool_error_text() {
    let direct = ReadToolCall::from_json("docker_logs", json!({"container": "a", "lines": 9999}))
        .unwrap_err();
    let reason = match direct {
        crate::triage::ToolArgError::InvalidArgs { reason, .. } => reason,
        other => panic!("{other}"),
    };
    let meta = MetaToolCall::from_json(
        "request_evidence",
        json!({"tool": "docker_logs", "args": {"container": "a", "lines": 9999}}),
    )
    .unwrap_err();
    assert!(meta.to_string().ends_with(&reason));
}

#[test]
fn plan_input_bounds_are_enforced() {
    let step = |extra: serde_json::Value| {
        let mut step = json!({"contract": "skill.a", "reason": "r"});
        for (key, value) in extra.as_object().unwrap() {
            step[key] = value.clone();
        }
        json!({"steps": [step]})
    };
    let many: serde_json::Map<String, serde_json::Value> =
        (0..33).map(|i| (format!("k{i}"), json!("v"))).collect();
    let cases = [
        json!({"steps": []}),
        json!({"steps": vec![json!({"contract": "skill.a", "reason": "r"}); 4]}),
        step(json!({"reason": "x".repeat(201)})),
        step(json!({"args": many})),
        step(json!({"args": {"a": 1.5}})),
        step(json!({"args": {"a": null}})),
        step(json!({"args": {"a": [1]}})),
        step(json!({"surprise": 1})),
    ];
    for case in cases {
        assert!(
            MetaToolCall::from_json("propose_plan", case.clone()).is_err(),
            "{case}"
        );
    }
}

#[test]
fn arg_values_are_canonical_strings() {
    assert_eq!(ArgValue::Text("a".into()).canonical(), "a");
    assert_eq!(ArgValue::Int(14).canonical(), "14");
    assert_eq!(ArgValue::Flag(true).canonical(), "true");
}

#[test]
fn from_proposals_builds_skill_steps() {
    assert!(PlanInput::from_proposals(&[]).is_none());
    let proposal = crate::ActionProposal {
        skill: crate::SkillName::parse("restart_container").unwrap(),
        args: [("container".to_owned(), "plex".to_owned())].into(),
        reason: "r".to_owned(),
    };
    let plan = PlanInput::from_proposals(&[proposal]).unwrap();
    assert_eq!(plan.steps[0].contract, "skill.restart_container");
}

#[test]
fn describe_capability_rejects_control_characters_and_long_ids() {
    for id in ["skill.a\nb".to_owned(), format!("skill.{}", "a".repeat(80))] {
        assert!(MetaToolCall::from_json("describe_capability", json!({ "contract": id })).is_err());
    }
}
