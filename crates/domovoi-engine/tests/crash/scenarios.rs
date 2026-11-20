//! The incident lifecycle every incident write goes through.
//!
//! It is deterministic: the clock only moves when a step moves it and the agent only
//! answers what the test scripted, so a re-run stops at exactly the same commit sequence up
//! to the crash. Every step is awaited before the next one starts, and a step that fails
//! while the process is still up is a test bug, not a crash.

use std::collections::BTreeMap;

use domovoi_core::{Incident, SignalClass, TriageOutcome, UserId, Verdict};
use domovoi_testkit::crash::SweepStore;
use jiff::SignedDuration;

use super::sweep::BoxFuture;
use super::world::{World, firing, host_id, resolved};

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
