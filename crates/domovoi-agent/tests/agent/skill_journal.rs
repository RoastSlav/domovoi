//! `run_skill` for skills that change the host: they are journaled, deduplicated and counted
//! by the mutation limiter, and a failed run is recorded as failed.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_agent::requests::{RequestCtx, run_skill};
use domovoi_config::MutationLimits;
use domovoi_core::ports::ProcessOutput;
use domovoi_core::{ExecutionId, PlanHash, Risk, SkillName};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    ActionState, ActionStatusQuery, CapabilityOutcome, RunSkill, SkillOutcome, SkillResult,
};
use domovoi_runner::SkillCatalog;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use crate::common::install;
use crate::package_rig::{Options, Rig};

fn name() -> SkillName {
    SkillName::parse("restart_app").unwrap()
}

fn args() -> BTreeMap<String, String> {
    BTreeMap::from([("service".to_owned(), "plex".to_owned())])
}

/// A rig whose catalogue holds one write skill and whose fake process exits with `exit`.
fn rig(limits: MutationLimits, exit: i32) -> Rig {
    let mut rig = Rig::new(
        &[],
        Options {
            section: false,
            allow_install: false,
            limits,
        },
    );
    install(&rig.fixture.skills(), "restart_app", "write");
    let (catalog, _) = SkillCatalog::load(&rig.fixture.skills());
    rig.ctx.catalog = Arc::new(catalog);
    rig.ctx.allow_risk = vec![Risk::Read, Risk::Write];
    let entry = rig
        .ctx
        .catalog
        .get(&name())
        .unwrap()
        .entry_path()
        .to_string_lossy()
        .into_owned();
    rig.fixture.exec.expect(
        &entry,
        &["plex"],
        ProcessOutput {
            exit_code: Some(exit),
            signal: None,
            stdout: "restarted".to_owned(),
            stderr: String::new(),
            truncated: false,
            timed_out: false,
            duration_ms: 3,
        },
    );
    rig
}

fn request(rig: &Rig) -> RunSkill {
    let loaded = rig.ctx.catalog.get(&name()).unwrap();
    let validated = loaded.manifest().validate_args(&args()).unwrap();
    RunSkill {
        action_id: ExecutionId::new(),
        skill: name(),
        args: args(),
        plan_hash: PlanHash::agent_check(
            &name(),
            loaded.manifest_hash(),
            rig.fixture.host_id,
            &validated,
        ),
        deadline: T0 + SignedDuration::from_mins(1),
    }
}

fn state(ctx: &RequestCtx, id: ExecutionId) -> ActionState {
    let journal = ctx.journal.clone().unwrap();
    journal
        .action_status(&ActionStatusQuery {
            execution_ids: vec![id.as_uuid()],
            expected_epoch: journal.epoch(),
        })
        .unwrap()
        .answers
        .remove(0)
        .state
}

#[tokio::test]
async fn approved_write_skill_runs_and_is_journaled_as_finished() {
    let rig = rig(MutationLimits::default(), 0);
    let run = request(&rig);
    let reply = run_skill(&rig.ctx, &run).await;
    let Message::SkillResult(SkillResult {
        outcome: SkillOutcome::Ok(out),
    }) = reply
    else {
        panic!("the skill did not run: {reply:?}");
    };
    assert_eq!(out.stdout, "restarted");
    assert_eq!(rig.fixture.exec.calls().len(), 1);
    let ActionState::Finished { result } = state(&rig.ctx, run.action_id) else {
        panic!("the journal has no finished record");
    };
    assert!(matches!(
        result.outcome,
        CapabilityOutcome::Succeeded { .. }
    ));
}

#[tokio::test]
async fn write_skill_that_exits_non_zero_is_journaled_as_failed() {
    let rig = rig(MutationLimits::default(), 1);
    let run = request(&rig);
    let reply = run_skill(&rig.ctx, &run).await;
    assert!(matches!(reply, Message::SkillResult(_)));
    let ActionState::Finished { result } = state(&rig.ctx, run.action_id) else {
        panic!("the journal has no finished record");
    };
    assert!(matches!(result.outcome, CapabilityOutcome::Failed { .. }));
}

#[tokio::test]
async fn replayed_execution_id_does_not_run_the_skill_twice() {
    let rig = rig(MutationLimits::default(), 0);
    let run = request(&rig);
    run_skill(&rig.ctx, &run).await;
    let replay = run_skill(&rig.ctx, &run).await;
    assert!(
        matches!(replay, Message::Error(_)),
        "a replay is answered as an error so the hub reconciles: {replay:?}"
    );
    assert_eq!(rig.fixture.exec.calls().len(), 1, "the skill ran twice");
}

#[tokio::test]
async fn mutation_limiter_refuses_a_write_skill_over_its_hourly_cap() {
    let rig = rig(
        MutationLimits {
            per_hour: 30,
            per_capability: BTreeMap::from([("skill.restart_app".to_owned(), 0)]),
        },
        0,
    );
    let run = request(&rig);
    let reply = run_skill(&rig.ctx, &run).await;
    assert!(matches!(reply, Message::Refused(_)), "{reply:?}");
    assert!(rig.fixture.exec.calls().is_empty(), "a refused skill ran");
}
