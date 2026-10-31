//! Tool and skill requests are re-validated locally, capped, redacted and bounded in time.

use std::sync::Arc;

use domovoi_agent::link::RequestKind;
use domovoi_agent::requests::{MAX_TOOL_OUTPUT, handle};
use domovoi_core::{ReadToolCall, Risk, Severity, SignalStatus};
use domovoi_proto::Message;
use domovoi_proto::messages::{RunCheck, RunTool, ToolFailureCode, ToolOutcome};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use crate::common::{Fixture, SECRET};
use crate::docker::FakeDocker;
use crate::harness::{ScriptedCheck, firing, key, ok, report, settle, target};
use crate::hub::guard;

fn tool(call: ReadToolCall) -> RequestKind {
    RequestKind::Tool(RunTool {
        call,
        deadline: T0 + SignedDuration::from_mins(1),
    })
}

fn outcome(message: Message) -> ToolOutcome {
    match message {
        Message::ToolResult(result) => result.outcome,
        other => panic!("expected tool_result, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_output_is_capped_and_redacted() {
    let fixture = Fixture::new();
    let logs = format!("token={SECRET}\n{}", "x".repeat(2 * MAX_TOOL_OUTPUT));
    let docker = Arc::new(FakeDocker { logs, hang: false });
    let ctx = fixture.ctx(vec![Risk::Read], Some(docker), Vec::new());
    let call = ReadToolCall::DockerLogs {
        container: "plex".to_owned(),
        lines: 100,
    };
    let ToolOutcome::Ok(success) = outcome(handle(&ctx, &tool(call)).await) else {
        panic!("the tool failed");
    };
    assert!(success.truncated);
    assert!(success.text.len() <= MAX_TOOL_OUTPUT);
    assert!(!success.text.contains(SECRET));
    assert!(success.text.starts_with("token=[REDACTED]"));
}

#[tokio::test]
async fn hub_only_tools_are_unknown() {
    let fixture = Fixture::new();
    let ctx = fixture.ctx(vec![Risk::Read], None, Vec::new());
    for call in [
        ReadToolCall::IncidentHistory { limit: 5 },
        ReadToolCall::NotesSearch {
            query: "disk".to_owned(),
            limit: 5,
        },
    ] {
        let ToolOutcome::Err(fail) = outcome(handle(&ctx, &tool(call)).await) else {
            panic!("a hub-only tool ran on the agent");
        };
        assert_eq!(fail.code, ToolFailureCode::UnknownTool);
    }
    // Docker tools without Docker are unavailable, not errors in the agent.
    let ps = tool(ReadToolCall::DockerPs { all: true });
    let ToolOutcome::Err(fail) = outcome(handle(&ctx, &ps).await) else {
        panic!("docker_ps ran without Docker");
    };
    assert_eq!(fail.code, ToolFailureCode::Unavailable);
    assert!(fixture.exec.calls().is_empty());
}

#[tokio::test]
async fn request_deadline_times_out() {
    let fixture = Fixture::new();
    let docker = Arc::new(FakeDocker {
        logs: String::new(),
        hang: true,
    });
    let ctx = Arc::new(fixture.ctx(vec![Risk::Read], Some(docker), Vec::new()));
    let request = RequestKind::Tool(RunTool {
        call: ReadToolCall::DockerLogs {
            container: "plex".to_owned(),
            lines: 10,
        },
        deadline: T0 + SignedDuration::from_secs(5),
    });
    let task = {
        let ctx = ctx.clone();
        tokio::spawn(async move { handle(&ctx, &request).await })
    };
    settle().await;
    fixture.clock.advance(SignedDuration::from_secs(5));
    let ToolOutcome::Err(fail) = outcome(guard(task).await.unwrap()) else {
        panic!("a hung tool returned");
    };
    assert_eq!(fail.code, ToolFailureCode::Timeout);
}

#[tokio::test]
async fn run_check_returns_observations_for_the_target() {
    let fixture = Fixture::new();
    let check = ScriptedCheck::new(
        "test.check",
        vec![report(vec![firing("a", Severity::Warning), ok("b")])],
    );
    let ctx = fixture.ctx(vec![Risk::Read], None, vec![Box::new(check)]);
    let request = RequestKind::Check(RunCheck {
        check: key("test.check"),
        target: target("a"),
        deadline: T0 + SignedDuration::from_mins(1),
    });
    let Message::CheckResult(result) = handle(&ctx, &request).await else {
        panic!("expected check_result");
    };
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.observations[0].target, target("a"));
    assert_eq!(result.observations[0].status, SignalStatus::Firing);

    let unknown = RequestKind::Check(RunCheck {
        check: key("no.such"),
        target: None,
        deadline: T0 + SignedDuration::from_mins(1),
    });
    assert!(matches!(handle(&ctx, &unknown).await, Message::Error(_)));
}

#[cfg(unix)]
mod skills {
    use std::collections::BTreeMap;

    use domovoi_agent::requests::run_skill;
    use domovoi_core::{ActionId, PlanHash, SkillName};
    use domovoi_proto::messages::{RunSkill, SkillFailureCode, SkillOutcome};

    use super::*;
    use crate::common::install;

    fn name(n: &str) -> SkillName {
        SkillName::parse(n).unwrap()
    }

    fn args(service: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("service".to_owned(), service.to_owned())])
    }

    /// A request carrying the plan hash the hub would compute for this host.
    fn request(
        fixture: &Fixture,
        ctx: &domovoi_agent::requests::RequestCtx,
        skill: &str,
        service: &str,
    ) -> RunSkill {
        let loaded = ctx.catalog.get(&name(skill)).unwrap();
        let validated = loaded.manifest().validate_args(&args(service)).unwrap();
        RunSkill {
            action_id: ActionId::new(),
            skill: name(skill),
            args: args(service),
            plan_hash: PlanHash::compute(
                &name(skill),
                loaded.manifest_hash(),
                fixture.host_id,
                &validated,
            ),
            deadline: T0 + SignedDuration::from_mins(1),
        }
    }

    fn code(outcome: SkillOutcome) -> SkillFailureCode {
        match outcome {
            SkillOutcome::Err(fail) => fail.code,
            SkillOutcome::Ok(_) => panic!("the skill ran"),
        }
    }

    #[tokio::test]
    async fn run_skill_rejects_unknown_skill_disallowed_risk_invalid_args_and_plan_mismatch() {
        let fixture = Fixture::new();
        install(&fixture.skills(), "restart_app", "write");
        install(&fixture.skills(), "wipe_cache", "destructive");
        let ctx = fixture.ctx(vec![Risk::Read, Risk::Write], None, Vec::new());
        let valid = request(&fixture, &ctx, "restart_app", "plex");

        // 1. Not in the local catalogue.
        let mut unknown = valid.clone();
        unknown.skill = name("rm_everything");
        assert_eq!(
            code(run_skill(&ctx, &unknown).await),
            SkillFailureCode::NotFound
        );
        // 2. Risk not allowed by policy.
        let destructive = request(&fixture, &ctx, "wipe_cache", "plex");
        assert_eq!(
            code(run_skill(&ctx, &destructive).await),
            SkillFailureCode::RiskNotAllowed
        );
        // 3. Arguments the manifest rejects, including an injection attempt.
        let mut bad_args = valid.clone();
        bad_args.args = args("plex; rm -rf /");
        assert_eq!(
            code(run_skill(&ctx, &bad_args).await),
            SkillFailureCode::InvalidArgs
        );
        // 4. Arguments changed after approval: the plan hash no longer matches.
        let mut swapped = valid.clone();
        swapped.args = args("nginx");
        assert_eq!(
            code(run_skill(&ctx, &swapped).await),
            SkillFailureCode::PlanMismatch
        );
        assert!(fixture.exec.calls().is_empty(), "the runner was called");

        // The valid request reaches the runner.
        let _ = run_skill(&ctx, &valid).await;
        assert_eq!(fixture.exec.calls().len(), 1);
    }

    #[tokio::test]
    async fn run_read_skill_tool_refuses_write_skills() {
        let fixture = Fixture::new();
        install(&fixture.skills(), "restart_app", "write");
        install(&fixture.skills(), "show_status", "read");
        let ctx = fixture.ctx(vec![Risk::Read, Risk::Write], None, Vec::new());
        let call = |skill: &str| {
            tool(ReadToolCall::RunReadSkill {
                skill: name(skill),
                args: args("plex"),
            })
        };
        let ToolOutcome::Err(fail) = outcome(handle(&ctx, &call("restart_app")).await) else {
            panic!("a write skill ran as a read tool");
        };
        assert_eq!(fail.code, ToolFailureCode::InvalidArgs);
        assert!(fixture.exec.calls().is_empty());

        let _ = handle(&ctx, &call("show_status")).await;
        assert_eq!(fixture.exec.calls().len(), 1);
    }
}
