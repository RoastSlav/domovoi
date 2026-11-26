//! Skill requests through the route: the two entry points share it, an unprivileged agent
//! never spawns a skill itself, and a root agent keeps the labelled legacy path.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use domovoi_agent::production::skill_route;
use domovoi_agent::requests::{run_read_skill, run_skill};
use domovoi_agent::skill_runner::SkillRoute;
use domovoi_config::{AgentConfig, MapEnv};
use domovoi_core::ports::ProcessOutput;
use domovoi_core::skill_runner::RunnerReply;
use domovoi_core::{ExecutionId, PlanHash, Risk, SkillName};
use domovoi_proto::messages::SkillRunnerStatus;
use domovoi_proto::Message;
use domovoi_proto::messages::{
    RunSkill, SkillFailureCode, SkillOutcome, SkillResult, ToolFailureCode,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FakeProcessExecutor};
use jiff::SignedDuration;

use crate::common::{Fixture, install};
use crate::runner_rig::{Action, always, run_reply};
use crate::skill_runner::{Rig, rig};

fn args() -> BTreeMap<String, String> {
    BTreeMap::from([("service".to_owned(), "plex".to_owned())])
}

fn reader() -> SkillName {
    SkillName::parse("reader").unwrap()
}

fn ctx_with_route(rig: &Rig, risks: Vec<Risk>) -> domovoi_agent::requests::RequestCtx {
    let mut ctx = rig.fixture.ctx(risks, None, Vec::new());
    ctx.skills = SkillRoute::Runner(rig.route.clone());
    ctx
}

fn run_request(ctx: &domovoi_agent::requests::RequestCtx) -> RunSkill {
    let skill = ctx.catalog.get(&reader()).unwrap();
    let validated = skill.manifest().validate_args(&args()).unwrap();
    RunSkill {
        action_id: ExecutionId::new(),
        skill: reader(),
        args: args(),
        plan_hash: PlanHash::agent_check(&reader(), skill.manifest_hash(), ctx.host_id, &validated),
        deadline: T0 + SignedDuration::from_mins(1),
    }
}

fn failure(reply: Message) -> (SkillFailureCode, String) {
    match reply {
        Message::SkillResult(SkillResult {
            outcome: SkillOutcome::Err(fail),
        }) => (fail.code, fail.message),
        other => panic!("the skill did not fail as a skill: {other:?}"),
    }
}

fn config() -> AgentConfig {
    let text = "[agent]\nhost_name = \"nas\"\nhub_url = \"wss://hub:7421\"\n";
    domovoi_config::parse::<AgentConfig>(text, &MapEnv::new())
        .unwrap()
        .0
}

#[tokio::test]
async fn production_route_is_legacy_only_for_euid_zero() {
    let clock = Arc::new(FakeClock::new(T0));
    let exec = Arc::new(FakeProcessExecutor::new());
    let root = skill_route(&config(), exec.clone(), clock.clone(), true);
    assert!(matches!(root, SkillRoute::Legacy(_)));
    let other = skill_route(&config(), exec, clock, false);
    assert!(matches!(other, SkillRoute::Runner(_)));
}

#[tokio::test]
async fn production_runner_route_expects_the_configured_roots_and_starts_absent() {
    let clock = Arc::new(FakeClock::new(T0));
    let exec = Arc::new(FakeProcessExecutor::new());
    let SkillRoute::Runner(route) = skill_route(&config(), exec, clock, false) else {
        panic!("not a runner route");
    };
    assert_eq!(route.state().status(), SkillRunnerStatus::Absent);
}

#[tokio::test]
async fn non_root_agent_never_spawns_skills_locally() {
    let rig = rig(always(Action::Reply(RunnerReply::Run(run_reply(
        "from runner",
    )))));
    let ctx = ctx_with_route(&rig, vec![Risk::Read]);

    // Runner not ok: the skill is unavailable, nothing is spawned.
    rig.route.state().set(SkillRunnerStatus::Absent);
    let fail = run_read_skill(&ctx, &reader(), &args()).await.unwrap_err();
    assert_eq!(fail.code, ToolFailureCode::Exec);
    assert_eq!(fail.message, "skill_runner_unavailable: absent");
    assert!(rig.fixture.exec.calls().is_empty());

    // Runner ok: the runner answers, still nothing is spawned in the agent.
    rig.route.state().set(crate::skill_runner::ok_status());
    let (text, _) = run_read_skill(&ctx, &reader(), &args()).await.unwrap();
    assert_eq!(text, "from runner");
    assert!(rig.fixture.exec.calls().is_empty());
}

#[tokio::test]
async fn run_as_unsupported_leads_the_failure_message() {
    let rig = rig(always(Action::Close));
    let path = rig.fixture.skills().join("reader/skill.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("entry = \"run\"\n", "entry = \"run\"\nrun_as = \"root\"\n"),
    )
    .unwrap();
    let ctx = ctx_with_route(&rig, vec![Risk::Read]);
    let fail = run_read_skill(&ctx, &reader(), &args()).await.unwrap_err();
    assert_eq!(fail.code, ToolFailureCode::Exec);
    assert_eq!(
        fail.message,
        "run_as_unsupported: reader names run_as root; only domovoi-skill is supported on this host"
    );
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn timed_out_reply_becomes_a_timeout_failure() {
    let mut late = run_reply("partial");
    late.timed_out = true;
    let rig = rig(always(Action::Reply(RunnerReply::Run(late))));
    let ctx = ctx_with_route(&rig, vec![Risk::Read]);
    let fail = run_read_skill(&ctx, &reader(), &args()).await.unwrap_err();
    assert_eq!(fail.code, ToolFailureCode::Timeout);
    let outcome = run_skill(&ctx, &run_request(&ctx)).await;
    assert_eq!(failure(outcome).0, SkillFailureCode::Timeout);
}

#[tokio::test]
async fn run_skill_and_read_skill_use_the_same_route() {
    let rig = rig(always(Action::Reply(RunnerReply::Run(run_reply("hi")))));
    let ctx = ctx_with_route(&rig, vec![Risk::Read]);
    run_read_skill(&ctx, &reader(), &args()).await.unwrap();
    let outcome = run_skill(&ctx, &run_request(&ctx)).await;
    assert!(
        matches!(
            outcome,
            Message::SkillResult(SkillResult {
                outcome: SkillOutcome::Ok(_)
            })
        ),
        "{outcome:?}"
    );
    assert_eq!(rig.runner.seen().len(), 2);
    assert!(rig.fixture.exec.calls().is_empty());
}

#[tokio::test]
async fn mutating_skill_without_a_journal_is_refused_before_the_route() {
    let rig = rig(always(Action::Reply(RunnerReply::Run(run_reply("no")))));
    install(&rig.fixture.skills(), "changer", "write");
    let mut ctx = rig
        .fixture
        .ctx(vec![Risk::Read, Risk::Write], None, Vec::new());
    ctx.skills = SkillRoute::Runner(rig.route.clone());
    let name = SkillName::parse("changer").unwrap();
    let skill = ctx.catalog.get(&name).unwrap();
    let validated = skill.manifest().validate_args(&args()).unwrap();
    let request = RunSkill {
        action_id: ExecutionId::new(),
        skill: name.clone(),
        args: args(),
        plan_hash: PlanHash::agent_check(&name, skill.manifest_hash(), ctx.host_id, &validated),
        deadline: T0 + SignedDuration::from_mins(1),
    };
    let (code, _) = failure(run_skill(&ctx, &request).await);
    assert_eq!(code, SkillFailureCode::Exec);
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn root_agent_keeps_legacy_run_as_switch() {
    let fixture = Fixture::new();
    install(&fixture.skills(), "reader", "read");
    let path = fixture.skills().join("reader/skill.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace(
            "entry = \"run\"\n",
            "entry = \"run\"\nrun_as = \"nobody\"\n",
        ),
    )
    .unwrap();
    let ctx = fixture.ctx(vec![Risk::Read], None, Vec::new());
    let skill = ctx.catalog.get(&reader()).unwrap();
    let entry = skill.entry_path().to_string_lossy().into_owned();
    fixture.exec.expect(
        &entry,
        &["plex"],
        ProcessOutput {
            exit_code: Some(0),
            signal: None,
            stdout: "legacy".to_owned(),
            stderr: String::new(),
            truncated: false,
            timed_out: false,
            duration_ms: 1,
        },
    );
    let (text, _) = run_read_skill(&ctx, &reader(), &args()).await.unwrap();
    assert_eq!(text, "legacy");
    let calls = fixture.exec.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].run_as.as_deref(),
        Some("nobody"),
        "the legacy path hands run_as to the executor, which switches user"
    );
}
