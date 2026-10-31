//! Invariant gates for the agent (INV-3, INV-4): the hub can only run skills that are in
//! this host's catalogue, and only exactly as approved.

#[cfg(test)]
#[path = "common/skills.rs"]
mod common;

use std::collections::BTreeMap;

use common::{Fixture, install};
use domovoi_agent::requests::run_skill;
use domovoi_core::{ActionId, PlanHash, Risk, SkillName};
use domovoi_proto::messages::{RunSkill, SkillFailureCode, SkillOutcome};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

#[cfg(test)]
fn failure(outcome: SkillOutcome) -> SkillFailureCode {
    match outcome {
        SkillOutcome::Err(fail) => fail.code,
        SkillOutcome::Ok(_) => panic!("the skill ran"),
    }
}

#[cfg(test)]
fn args() -> BTreeMap<String, String> {
    BTreeMap::from([("service".to_owned(), "plex".to_owned())])
}

/// INV-3: a skill that is not in the local catalogue never reaches the runner, whatever
/// the hub sends, including a plan hash it computed for a manifest of its own.
#[tokio::test]
async fn hub_cannot_run_skill_not_in_local_catalog() {
    let fixture = Fixture::new();
    install(&fixture.skills(), "restart_app", "write");
    let ctx = fixture.ctx(
        vec![Risk::Read, Risk::Write, Risk::Destructive],
        None,
        Vec::new(),
    );
    let pushed = domovoi_testkit::build::manifest("hub_pushed", Risk::Write).unwrap();
    let validated = pushed.validate_args(&BTreeMap::new()).unwrap();
    for skill in ["hub_pushed", "../restart_app", "restart_app/../x"] {
        let Ok(skill) = SkillName::parse(skill) else {
            continue;
        };
        let request = RunSkill {
            action_id: ActionId::new(),
            plan_hash: PlanHash::compute(
                &skill,
                pushed.manifest_hash(),
                fixture.host_id,
                &validated,
            ),
            skill,
            args: args(),
            deadline: T0 + SignedDuration::from_mins(1),
        };
        assert_eq!(
            failure(run_skill(&ctx, &request).await),
            SkillFailureCode::NotFound
        );
    }
    assert!(fixture.exec.calls().is_empty(), "the runner was called");
}

/// INV-4: a plan hash that does not match this host's manifest, host and arguments never
/// executes.
#[cfg(unix)]
#[tokio::test]
async fn plan_hash_mismatch_never_executes() {
    let fixture = Fixture::new();
    install(&fixture.skills(), "restart_app", "write");
    let ctx = fixture.ctx(vec![Risk::Read, Risk::Write], None, Vec::new());
    let skill = SkillName::parse("restart_app").unwrap();
    let loaded = ctx.catalog.get(&skill).unwrap();
    let validated = loaded.manifest().validate_args(&args()).unwrap();
    let other_host = domovoi_core::HostId::new();
    let other_args = loaded
        .manifest()
        .validate_args(&BTreeMap::from([(
            "service".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let mut changed = loaded.manifest().clone();
    changed.description = "edited after approval".to_owned();
    let wrong = [
        PlanHash::compute(&skill, loaded.manifest_hash(), other_host, &validated),
        PlanHash::compute(&skill, loaded.manifest_hash(), fixture.host_id, &other_args),
        PlanHash::compute(&skill, changed.manifest_hash(), fixture.host_id, &validated),
    ];
    for plan_hash in wrong {
        let request = RunSkill {
            action_id: ActionId::new(),
            skill: skill.clone(),
            args: args(),
            plan_hash,
            deadline: T0 + SignedDuration::from_mins(1),
        };
        assert_eq!(
            failure(run_skill(&ctx, &request).await),
            SkillFailureCode::PlanMismatch
        );
    }
    assert!(fixture.exec.calls().is_empty(), "the runner was called");
}
