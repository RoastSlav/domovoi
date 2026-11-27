//! Read skills behind the skill runner: a declared need is answered by the agent's built-in
//! read or refused with the reason, and a skill that returns no data is a failure.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use domovoi_agent::requests::{RequestCtx, run_read_skill, run_tool};
use domovoi_agent::skill_runner::SkillRoute;
use domovoi_checks::commands::{Commands, DayPriority, ServiceState, ToolHost};
use domovoi_core::ports::{ProcessOutput, ProcessSpec};
use domovoi_core::skill_runner::RunnerReply;
use domovoi_core::{HostNeed, ReadToolCall, Risk, SkillName};
use domovoi_proto::messages::ToolFailureCode;
use domovoi_testkit::FixtureHostFs;

use crate::common::install;
use crate::runner_rig::{Action, always, run_reply};
use crate::skill_runner::{Rig, ok_status, rig};

const HINT: &str = "Hint: You are currently not seeing messages from other users and the system.";

/// A context behind a ready runner, with the host tools found, and the skill `reader` with
/// `extra` manifest lines.
async fn context(rig: &Rig, extra: &str) -> RequestCtx {
    let skills = rig.fixture.skills();
    install(&skills, "reader", "read");
    let path = skills.join("reader/skill.toml");
    let manifest = std::fs::read_to_string(&path).unwrap();
    // These skills take no parameter.
    let manifest = manifest.split("\n[[params]]").next().unwrap();
    std::fs::write(&path, format!("{manifest}{extra}")).unwrap();
    let tools = rig.fixture.dir.path().join("tools");
    for program in ["journalctl", "systemctl", "ps"] {
        std::fs::create_dir_all(tools.join("usr/bin")).unwrap();
        std::fs::write(tools.join("usr/bin").join(program), "").unwrap();
    }
    let mut ctx = rig.fixture.ctx(vec![Risk::Read], None, Vec::new());
    ctx.commands = Arc::new(Commands::resolve(&FixtureHostFs::new(tools), ToolHost::Host).await);
    rig.route.state().set(ok_status());
    ctx.skills = SkillRoute::Runner(rig.route.clone());
    ctx
}

fn answer(rig: &Rig, spec: &ProcessSpec, stdout: &str, stderr: &str) {
    let args: Vec<&str> = spec.args.iter().map(String::as_str).collect();
    rig.fixture.exec.expect(
        &spec.program,
        &args,
        ProcessOutput {
            exit_code: Some(0),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            ..Default::default()
        },
    );
}

fn reader() -> SkillName {
    SkillName::parse("reader").unwrap()
}

async fn read(ctx: &RequestCtx) -> Result<String, (ToolFailureCode, String)> {
    run_read_skill(ctx, &reader(), &BTreeMap::new())
        .await
        .map(|(text, _)| text)
        .map_err(|fail| (fail.code, fail.message))
}

#[tokio::test]
async fn a_declared_built_in_read_is_answered_by_the_agent_with_real_output() {
    let rig = rig(always(Action::Close));
    let ctx = context(
        &rig,
        "\nneeds = [\"services\"]\nagent_read = \"services\"\n",
    )
    .await;
    let c = &ctx.commands;
    answer(
        &rig,
        &c.systemctl_services(ServiceState::Running).unwrap(),
        "ssh.service loaded active running OpenSSH\n",
        "",
    );
    answer(
        &rig,
        &c.systemctl_services(ServiceState::Failed).unwrap(),
        "smartmontools.service loaded failed failed SMART\n",
        "",
    );
    answer(&rig, &c.systemctl_timers().unwrap(), "", "");
    let text = read(&ctx).await.unwrap();
    assert!(text.contains("the entry script did not run"), "{text}");
    assert!(
        text.contains("== failed services\nsmartmontools.service"),
        "{text}"
    );
    assert!(text.contains("== timers\n(none)"), "{text}");
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_need_nothing_answers_is_refused_with_the_reason_and_announced() {
    let rig = rig(always(Action::Close));
    let ctx = context(&rig, "\nneeds = [\"files\"]\n").await;
    let (_, message) = read(&ctx).await.unwrap_err();
    assert!(message.starts_with("host_view_hidden:"), "{message}");
    assert!(message.contains("/home"), "{message}");
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 0);
    let summaries = ctx.announced_skills().await;
    assert_eq!(summaries[0].hidden_needs, [HostNeed::Files]);
}

#[tokio::test]
async fn an_account_that_cannot_read_the_journal_is_announced_and_never_reads_as_empty() {
    let rig = rig(always(Action::Close));
    let ctx = context(
        &rig,
        "\nneeds = [\"journal\"]\nagent_read = \"log_summary\"\n",
    )
    .await;
    answer(
        &rig,
        &ctx.commands.journal(None, 60, 1, false, None).unwrap(),
        "",
        HINT,
    );
    let summaries = ctx.announced_skills().await;
    assert!(summaries[0].hidden_needs.is_empty());
    assert!(summaries[0].agent_journal_unreadable);

    let spec = ctx
        .commands
        .journal(None, 60, 5, false, Some("err"))
        .unwrap();
    assert!(spec.args.contains(&"--priority=err".to_owned()));
    assert!(
        !spec.args.contains(&"--quiet".to_owned()),
        "{:?}",
        spec.args
    );
    answer(&rig, &spec, "", HINT);
    let call = ReadToolCall::Journal {
        unit: None,
        since_minutes: 60,
        lines: 5,
        kernel: false,
        priority: Some("err".to_owned()),
    };
    let fail = run_tool(&ctx, &call).await.unwrap_err();
    assert_eq!(fail.code, ToolFailureCode::Unavailable);
    assert!(fail.message.contains("systemd-journal"), "{}", fail.message);
}

/// Long journal entries can fill the output limit, which stops journalctl; what it printed
/// is still an answer.
#[tokio::test]
async fn a_summary_cut_at_the_output_limit_still_answers() {
    let rig = rig(always(Action::Close));
    let ctx = context(
        &rig,
        "\nneeds = [\"journal\"]\nagent_read = \"log_summary\"\n",
    )
    .await;
    let c = &ctx.commands;
    answer(
        &rig,
        &c.journal_day(DayPriority::Warnings, 5000).unwrap(),
        "sshd\n",
        "",
    );
    let errors = c.journal_day(DayPriority::Errors, 20).unwrap();
    let args: Vec<&str> = errors.args.iter().map(String::as_str).collect();
    rig.fixture.exec.expect(
        &errors.program,
        &args,
        ProcessOutput {
            stdout: "a long error\n".to_owned(),
            truncated: true,
            ..Default::default()
        },
    );
    let text = read(&ctx).await.unwrap();
    assert!(text.contains("      1 sshd"), "{text}");
    assert!(
        text.contains("a long error\n(cut at the output limit)"),
        "{text}"
    );
}

#[tokio::test]
async fn headings_without_data_are_a_failure_not_an_empty_result() {
    let rig = rig(always(Action::Reply(RunnerReply::Run(run_reply(
        "== running services\n\n== failed services\n",
    )))));
    let ctx = context(&rig, "\n").await;
    let (code, message) = read(&ctx).await.unwrap_err();
    assert_eq!(code, ToolFailureCode::Exec);
    assert!(message.contains("returned no data"), "{message}");
}
