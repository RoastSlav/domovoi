//! The two lifecycles every write goes through: an incident's, and an action's.
//!
//! Both are deterministic: the clock only moves when a step moves it and the agent only
//! answers what the test scripted, so a re-run stops at exactly the same commit sequence up
//! to the crash. Every step is awaited before the next one starts, and a step that fails
//! while the process is still up is a test bug, not a crash.

use std::collections::BTreeMap;

use domovoi_core::ports::Presence;
use domovoi_core::{
    ActionStatus, Incident, ManifestHash, SignalClass, SkillSummary, TriageOutcome, UserId, Verdict,
};
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_testkit::approvals::{chat, human};
use domovoi_testkit::crash::SweepStore;
use jiff::SignedDuration;

use super::sweep::BoxFuture;
use super::world::{
    World, all_skills, check, fail_skill, firing, host_id, loop_skill, mutable_skill,
    offline_skill, resolved, verified_restart, verify_fail_skill,
};

/// Awaits one step. After a crash the scenario stops, because nothing it does can be
/// stored any more; before one, a failed step is a test bug and stops the test.
macro_rules! step {
    ($world:expr, $name:expr, $e:expr) => {
        match $e.await {
            Ok(value) => value,
            Err(err) => {
                if !$world.crashing.crashed() {
                    panic!("{}: a step failed before any crash: {err}", $name);
                }
                return;
            }
        }
    };
}

const HOUR: SignedDuration = SignedDuration::from_hours(1);
const ARGS: &[(&str, &str)] = &[("container", "nginx")];

/// One incident from its first signal to its last write: opening, folding, the human
/// commands, triage, a follow-up, a verdict, resolving, reopening, dismissing, a pending
/// incident discarded before confirmation, and a tick that resolves a quiet one.
pub fn incident_lifecycle<S: SweepStore>(world: &mut World<S>) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        const NAME: &str = "incident_lifecycle";
        let user = UserId::new();
        let key = "app.health";
        let target = Some("api");

        // 1. An immediate firing signal opens the incident.
        step!(
            world,
            NAME,
            world.manager.ingest(vec![firing(
                host_id(),
                key,
                target,
                SignalClass::Immediate,
                world.now()
            )])
        );
        let id = world
            .incidents()
            .await
            .iter()
            .find(|incident| incident.check().as_str() == key)
            .map(Incident::id)
            .expect("the immediate signal opened an incident");

        // 2. The same signal again only folds into the occurrence count.
        step!(
            world,
            NAME,
            world.manager.ingest(vec![firing(
                host_id(),
                key,
                target,
                SignalClass::Immediate,
                world.now()
            )])
        );

        // 3. Acknowledged, muted and unmuted by a person.
        step!(world, NAME, world.manager.ack(id, user));
        let until = world
            .now()
            .checked_add(SignedDuration::from_secs(60))
            .expect("a mute deadline");
        step!(world, NAME, world.manager.mute(id, until, user));
        step!(world, NAME, world.manager.unmute(id));

        // 4. Triage finished without a report.
        step!(
            world,
            NAME,
            world.manager.attach_triage(
                id,
                TriageOutcome::Skipped {
                    reason: "the harness runs no triage".to_owned(),
                },
                BTreeMap::new(),
            )
        );

        // 5. A follow-up answer and a verdict.
        step!(
            world,
            NAME,
            world.manager.record_follow_up(
                id,
                user,
                "why did it restart?".to_owned(),
                "a bad config file".to_owned(),
                world.now(),
                0,
            )
        );
        step!(world, NAME, world.manager.set_verdict(id, Verdict::Fixed));

        // 6. A resolved signal closes it.
        step!(
            world,
            NAME,
            world
                .manager
                .ingest(vec![resolved(host_id(), key, target, world.now())])
        );

        // 7. A firing signal inside the reopen window reopens it.
        step!(
            world,
            NAME,
            world.manager.ingest(vec![firing(
                host_id(),
                key,
                target,
                SignalClass::Immediate,
                world.now()
            )])
        );

        // 8. A person dismisses it as a known condition.
        step!(
            world,
            NAME,
            world
                .manager
                .dismiss(id, user, "known condition".to_owned())
        );

        // 9. A normal-class signal leaves a pending incident, and its resolve discards it.
        step!(
            world,
            NAME,
            world.manager.ingest(vec![firing(
                host_id(),
                "net.link",
                Some("eth0"),
                SignalClass::Normal,
                world.now()
            )])
        );
        step!(
            world,
            NAME,
            world.manager.ingest(vec![resolved(
                host_id(),
                "net.link",
                Some("eth0"),
                world.now()
            )])
        );

        // 10. Past the mute and past the quiet TTL: the tick resolves the seeded incident.
        world.advance(11 * 60);
        step!(world, NAME, world.manager.tick());
    })
}

/// One action from proposal to every status the writer writes: a chat approval after a token
/// rotation, a denial, an expiry, a failed run, a failed verification, a dispatch rejected
/// for a changed plan, the loop guard blocking a third run, and an offline agent.
pub fn action_lifecycle<S: SweepStore>(world: &mut World<S>) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        const NAME: &str = "action_lifecycle";
        let user = UserId::new();
        let incident = world
            .incidents()
            .await
            .iter()
            .find(|incident| incident.check().as_str() == "docker.container")
            .map(Incident::id)
            .expect("the seeded incident is there");

        // 1. Two proposals for the same skill.
        let (p1, _) = step!(
            world,
            NAME,
            world.propose(incident, &verified_restart(), ARGS, HOUR)
        );
        let (p2, _) = step!(
            world,
            NAME,
            world.propose(incident, &verified_restart(), ARGS, HOUR)
        );

        // 2. The chat token is rotated, the new one approves, and the run verifies.
        let rotated = mint_token();
        step!(
            world,
            NAME,
            world
                .service
                .rotate_chat_token(p1.id(), token_hash(&rotated))
        );
        step!(
            world,
            NAME,
            world
                .service
                .approve(p1.id(), chat(user, "discord", "raiko"), Some(&rotated))
        );
        step!(
            world,
            NAME,
            world.wait_for(p1.id(), |status| matches!(status, ActionStatus::Verified))
        );
        // The verification's trailing verdict write is the run's last commit, so wait for
        // it before the next step, or the two runs' commit orders could differ.
        step!(
            world,
            NAME,
            world.wait_for_verdict(incident, Verdict::Fixed)
        );

        // 3. A denial.
        step!(world, NAME, world.service.deny(p2.id(), human(user)));

        // 4. An approval window that closes.
        let (p3, _) = step!(
            world,
            NAME,
            world.propose(incident, &loop_skill(), ARGS, SignedDuration::from_secs(1))
        );
        world.advance(2);
        let expired = step!(world, NAME, world.service.expire_due());
        assert_eq!(
            expired, 1,
            "exactly the proposal whose window closed expires"
        );
        let stored = world
            .action(p3.id())
            .await
            .expect("the expired proposal is stored");
        assert_eq!(stored.status(), &ActionStatus::Expired);

        // 5. An approval whose run fails.
        let (p4, _) = step!(
            world,
            NAME,
            world.propose(incident, &fail_skill(), ARGS, HOUR)
        );
        step!(
            world,
            NAME,
            world.service.approve(p4.id(), human(user), None)
        );
        step!(
            world,
            NAME,
            world.wait_for(p4.id(), |status| matches!(
                status,
                ActionStatus::Failed { .. }
            ))
        );

        // 6. A run that succeeds but whose re-check still finds the problem.
        world
            .link
            .on_check(|_, _, _| Ok(check(domovoi_core::SignalStatus::Firing)));
        let (p5, _) = step!(
            world,
            NAME,
            world.propose(incident, &verify_fail_skill(), ARGS, HOUR)
        );
        step!(
            world,
            NAME,
            world.service.approve(p5.id(), human(user), None)
        );
        step!(
            world,
            NAME,
            world.wait_for(p5.id(), |status| matches!(
                status,
                ActionStatus::VerifyFailed
            ))
        );
        world
            .link
            .on_check(|_, _, _| Ok(check(domovoi_core::SignalStatus::Resolved)));

        // 7. The plan the host offers changes between approval and dispatch.
        let changed = rehashed(mutable_skill());
        world
            .link
            .set_skills(host_id(), replaced(all_skills(), changed));
        let (p6, _) = step!(
            world,
            NAME,
            world.propose(incident, &mutable_skill(), ARGS, HOUR)
        );
        step!(
            world,
            NAME,
            world.service.approve(p6.id(), human(user), None)
        );
        step!(
            world,
            NAME,
            world.wait_for(p6.id(), |status| matches!(
                status,
                ActionStatus::Rejected { .. }
            ))
        );
        world.link.set_skills(host_id(), all_skills());

        // 8. Two runs use the hour's slots; the third is blocked before it dispatches.
        for _ in 0..2 {
            let (action, _) = step!(
                world,
                NAME,
                world.propose(incident, &loop_skill(), ARGS, HOUR)
            );
            step!(
                world,
                NAME,
                world.service.approve(action.id(), human(user), None)
            );
            step!(
                world,
                NAME,
                world.wait_for(action.id(), |status| matches!(
                    status,
                    ActionStatus::Succeeded
                ))
            );
        }
        let (blocked, _) = step!(
            world,
            NAME,
            world.propose(incident, &loop_skill(), ARGS, HOUR)
        );
        step!(
            world,
            NAME,
            world.service.approve(blocked.id(), human(user), None)
        );
        step!(
            world,
            NAME,
            world.wait_for(blocked.id(), |status| matches!(
                status,
                ActionStatus::Rejected { .. }
            ))
        );

        // 9. Approved while the agent is offline, with no wait configured.
        world.link.set_presence(
            host_id(),
            Presence::Disconnected {
                last_seen: Some(world.now()),
            },
        );
        let (p10, _) = step!(
            world,
            NAME,
            world.propose(incident, &offline_skill(), ARGS, HOUR)
        );
        step!(
            world,
            NAME,
            world.service.approve(p10.id(), human(user), None)
        );
        step!(
            world,
            NAME,
            world.wait_for(p10.id(), |status| matches!(
                status,
                ActionStatus::Rejected { .. }
            ))
        );
    })
}

/// A skill whose plan hash no longer matches what a proposal bound to.
fn rehashed(mut skill: SkillSummary) -> SkillSummary {
    skill.manifest_hash = "cd"
        .repeat(32)
        .parse::<ManifestHash>()
        .expect("a valid manifest hash");
    skill
}

/// `skills` with the entry named like `replacement` replaced.
fn replaced(skills: Vec<SkillSummary>, replacement: SkillSummary) -> Vec<SkillSummary> {
    skills
        .into_iter()
        .map(|skill| {
            if skill.name == replacement.name {
                replacement.clone()
            } else {
                skill
            }
        })
        .collect()
}
