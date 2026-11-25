//! The agent's route to the skill runner, against a fake runner on a socket in a temporary
//! directory. Time is the fake clock; nothing sleeps.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use domovoi_agent::skill_runner::{MAX_CONCURRENT_RUNS, RouteError, RunnerRoute};
use domovoi_core::skill_runner::{RefusalCode, RunnerReply, RunnerRequest, SkillSource};
use domovoi_core::{ImplHash, SkillName, ValidatedArgs};
use domovoi_proto::messages::SkillRunnerStatus;
use domovoi_runner::{LoadedSkill, SkillCatalog};
use futures::FutureExt;
use tokio::sync::Notify;

use crate::common::{Fixture, install};
use crate::runner_rig::{Action, FakeRunner, Script, always, refused, run_reply};

/// A fixture with a read skill `reader`, a fake runner and a route in the `Ok` state.
pub struct Rig {
    pub fixture: Fixture,
    pub runner: FakeRunner,
    pub route: Arc<RunnerRoute>,
    pub skill: LoadedSkill,
    pub args: ValidatedArgs,
}

pub fn ok_status() -> SkillRunnerStatus {
    SkillRunnerStatus::Ok {
        version: "1.2.3".to_owned(),
    }
}

pub fn rig(script: Script) -> Rig {
    let fixture = Fixture::new();
    install(&fixture.skills(), "reader", "read");
    let run_dir = fixture.dir.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    let runner = FakeRunner::start(&run_dir, script);
    let route = runner.route(
        fixture.clock.clone(),
        &fixture.skills(),
        Path::new("/var/lib/domovoi-packages"),
    );
    route.state().set(ok_status());
    let (catalog, _) = SkillCatalog::load(&fixture.skills());
    let skill = catalog
        .get(&SkillName::parse("reader").unwrap())
        .unwrap()
        .clone();
    let args = skill
        .manifest()
        .validate_args(&BTreeMap::from([("service".to_owned(), "plex".to_owned())]))
        .unwrap();
    Rig {
        fixture,
        runner,
        route,
        skill,
        args,
    }
}

/// Lets other tasks run until `done` holds.
pub async fn until(done: impl Fn() -> bool) {
    for _ in 0..2000 {
        if done() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the condition never held");
}

type Handle = tokio::task::JoinHandle<Result<domovoi_core::ports::SkillOutput, RouteError>>;

pub fn spawn_run(rig: &Rig) -> Handle {
    let (route, skill, args) = (rig.route.clone(), rig.skill.clone(), rig.args.clone());
    tokio::spawn(async move { route.run(&skill, &args).await })
}

fn reply(text: &str) -> Action {
    Action::Reply(RunnerReply::Run(run_reply(text)))
}

#[tokio::test]
async fn route_sends_one_request_and_keeps_write_side_open_until_reply() {
    let gate = Arc::new(Notify::new());
    let held = Action::Gated(gate.clone(), RunnerReply::Run(run_reply("done")));
    let rig = rig(always(held));
    let handle = spawn_run(&rig);
    until(|| !rig.runner.seen().is_empty()).await;
    gate.notify_one();
    let output = handle.await.unwrap().unwrap();
    assert_eq!(output.stdout, "done");
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 1);
    let seen = rig.runner.seen();
    assert_eq!(seen.len(), 1);
    let RunnerRequest::Run(request) = &seen[0] else {
        panic!("not a run request: {:?}", seen[0]);
    };
    assert_eq!(request.source, SkillSource::SkillsDir);
    assert_eq!(request.skill.as_str(), "reader");
    assert_eq!(request.timeout_secs, 10);
    assert_eq!(request.max_output_bytes, 4096);
    assert_eq!(request.args["service"], "plex");
    assert_eq!(
        request.impl_hash,
        ImplHash::from_parts(rig.skill.manifest_hash(), rig.skill.entry_script_digest())
    );
    assert_eq!(
        *rig.runner.open_at_reply.lock().unwrap(),
        vec![true],
        "the write side stayed open until the reply was written"
    );
}

#[tokio::test]
async fn dropping_the_run_future_closes_the_connection() {
    let rig = rig(always(Action::Stall));
    let handle = spawn_run(&rig);
    until(|| !rig.runner.seen().is_empty()).await;
    assert_eq!(rig.runner.peer_closed.load(Ordering::SeqCst), 0);
    handle.abort();
    until(|| rig.runner.peer_closed.load(Ordering::SeqCst) == 1).await;
    assert!(
        rig.route.state().is_ok(),
        "a cancel is not a runner failure"
    );
}

#[tokio::test]
async fn route_maps_reply_to_skill_output_and_recaps_streams() {
    let mut big = run_reply(&"a".repeat(10_000));
    big.stderr = "e".repeat(5_000);
    big.exit_code = Some(3);
    let rig = rig(always(Action::Reply(RunnerReply::Run(big))));
    let output = rig.route.run(&rig.skill, &rig.args).await.unwrap();
    assert_eq!(output.stdout.len(), 4096);
    assert_eq!(output.stderr.len(), 4096);
    assert!(output.truncated);
    assert_eq!(output.exit_code, Some(3));
    assert_eq!(output.duration_ms, 12);
    assert_eq!(output.argv_hash, [0xab; 32]);
}

#[tokio::test]
async fn route_maps_timed_out_reply_to_timeout_failure() {
    let mut late = run_reply("partial");
    late.timed_out = true;
    late.exit_code = None;
    late.signal = Some(15);
    let rig = rig(always(Action::Reply(RunnerReply::Run(late))));
    match rig.route.run(&rig.skill, &rig.args).await {
        Err(RouteError::TimedOut(output)) => assert_eq!(output.stdout, "partial"),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn route_maps_a_runner_refusal_without_marking_the_runner_broken() {
    let rig = rig(always(Action::Reply(refused(
        RefusalCode::ImplHashMismatch,
        "the skill changed",
    ))));
    match rig.route.run(&rig.skill, &rig.args).await {
        Err(RouteError::Refused { code, detail }) => {
            assert_eq!(code, RefusalCode::ImplHashMismatch);
            assert_eq!(detail, "the skill changed");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(rig.route.state().is_ok());
}

#[tokio::test]
async fn route_refuses_run_as_before_connecting() {
    let rig = rig(always(reply("x")));
    let manifest = std::fs::read_to_string(rig.fixture.skills().join("reader/skill.toml")).unwrap();
    for (user, allowed) in [("root", false), ("domovoi-skill", true)] {
        let edited = manifest.replace(
            "entry = \"run\"\n",
            &format!("entry = \"run\"\nrun_as = \"{user}\"\n"),
        );
        std::fs::write(rig.fixture.skills().join("reader/skill.toml"), edited).unwrap();
        let (catalog, issues) = SkillCatalog::load(&rig.fixture.skills());
        assert!(issues.is_empty(), "{issues:?}");
        let skill = catalog
            .get(&SkillName::parse("reader").unwrap())
            .unwrap()
            .clone();
        let before = rig.runner.accepted.load(Ordering::SeqCst);
        let result = rig.route.run(&skill, &rig.args).await;
        let after = rig.runner.accepted.load(Ordering::SeqCst);
        if allowed {
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(after, before + 1);
        } else {
            assert_eq!(
                result.unwrap_err(),
                RouteError::RunAsUnsupported {
                    run_as: user.to_owned()
                }
            );
            assert_eq!(after, before, "the runner saw no connection");
        }
    }
}

#[tokio::test]
async fn route_refuses_when_status_is_not_ok_and_never_runs_locally() {
    let rig = rig(always(reply("x")));
    for (status, reason) in [
        (SkillRunnerStatus::Absent, "absent"),
        (
            SkillRunnerStatus::Error {
                reason: "runner_unreachable".to_owned(),
            },
            "runner_unreachable",
        ),
        (
            SkillRunnerStatus::Unsupported {
                reason: "runner_privileges".to_owned(),
            },
            "runner_privileges",
        ),
    ] {
        rig.route.state().set(status);
        let err = rig.route.run(&rig.skill, &rig.args).await.unwrap_err();
        assert_eq!(err, RouteError::RunnerUnavailable(reason.to_owned()));
        assert_eq!(
            err.to_string(),
            format!("skill_runner_unavailable: {reason}")
        );
    }
    assert_eq!(rig.runner.accepted.load(Ordering::SeqCst), 0);
    assert!(
        rig.fixture.exec.calls().is_empty(),
        "nothing was spawned in the agent"
    );
}

#[tokio::test]
async fn route_limits_concurrency_to_eight() {
    let rig = rig(always(Action::Stall));
    let handles: Vec<Handle> = (0..=MAX_CONCURRENT_RUNS).map(|_| spawn_run(&rig)).collect();
    until(|| rig.runner.accepted.load(Ordering::SeqCst) >= MAX_CONCURRENT_RUNS).await;
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        rig.runner.accepted.load(Ordering::SeqCst),
        MAX_CONCURRENT_RUNS,
        "the ninth run waits for a permit"
    );
    handles[0].abort();
    until(|| rig.runner.accepted.load(Ordering::SeqCst) == MAX_CONCURRENT_RUNS + 1).await;
    for handle in handles {
        handle.abort();
    }
}

#[tokio::test]
async fn route_marks_error_and_wakes_probe_on_transport_failure() {
    let rig = rig(always(Action::Close));
    let err = rig.route.run(&rig.skill, &rig.args).await.unwrap_err();
    assert_eq!(
        err,
        RouteError::RunnerUnavailable("runner_unreachable".to_owned())
    );
    assert_eq!(
        rig.route.state().status(),
        SkillRunnerStatus::Error {
            reason: "runner_unreachable".to_owned()
        }
    );
    assert!(
        rig.route.state().woken().now_or_never().is_some(),
        "the probe task was woken"
    );
}
