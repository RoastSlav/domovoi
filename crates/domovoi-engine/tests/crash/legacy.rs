//! The retirement of legacy actions, crashed at every commit: an open proposal, an approval
//! and a dispatched run left by the first-generation executor are each settled in one
//! transaction, so a crash leaves each either as it was or fully settled.

use std::sync::Arc;

use domovoi_core::ports::{
    ActionGuard, AggregateRef, AuditDraft, EffectQueue, JournalDraft, JournalKind, StateWrite,
    Transaction, UnitOfWork,
};
use domovoi_core::{
    ActionRequest, Actor, ApprovalProvenance, Authorization, Incident, PlanBinding, Risk, UserId,
};
use domovoi_engine::legacy::{LegacyDrainDeps, drain_legacy_actions};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{action, at};
use domovoi_testkit::crash::SweepStore;
use serde_json::json;

use super::sweep::BoxFuture;
use super::world::{World, host_id};

/// The audit kind a legacy action's last status was recorded under.
fn audit_kind(status: &str) -> String {
    format!("action.{status}")
}

/// A transaction that stores `action` as the first-generation executor left it, with the
/// audit row and journal row it wrote for its last status.
fn left_behind(action: &ActionRequest) -> Transaction {
    let status = action.status().name();
    let who = Actor::System("executor".to_owned());
    Transaction::new()
        .write(StateWrite::Action {
            action: action.clone(),
            expected: ActionGuard::New,
        })
        .audit(AuditDraft::new(
            who.clone(),
            action.updated_at(),
            audit_kind(status),
            format!("action:{}", action.id()),
            json!({}),
        ))
        .journal(JournalDraft {
            aggregate: AggregateRef::action(action.id()),
            kind: JournalKind::ActionTransition,
            payload_v: 1,
            payload: json!({ "to": status }),
            actor: who,
            occurred_at: action.updated_at(),
        })
}

/// Three legacy actions, one in each open status, and the drain that retires them.
pub fn legacy_drain<S: SweepStore + EffectQueue + 'static>(
    world: &mut World<S>,
) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        let incident = world
            .incidents()
            .await
            .iter()
            .find(|incident| incident.check().as_str() == "docker.container")
            .map(Incident::id)
            .expect("the seeded incident is there");

        let proposed = action(incident, host_id(), Risk::Write).expect("a valid action");
        let mut approved = action(incident, host_id(), Risk::Write).expect("a valid action");
        approve(&mut approved);
        let mut dispatched = action(incident, host_id(), Risk::Write).expect("a valid action");
        approve(&mut dispatched);
        let hash = dispatched.plan_hash_for_approval();
        dispatched
            .dispatch(hash, None, at(20))
            .expect("an approved action dispatches");

        for legacy in [&proposed, &approved, &dispatched] {
            if world.crashing.commit(left_behind(legacy)).await.is_err() {
                assert!(
                    world.crashing.crashed(),
                    "seeding a legacy action failed before any crash"
                );
                return;
            }
        }

        world.advance(100);
        let store = world.store.clone();
        let deps = LegacyDrainDeps {
            actions: store.clone(),
            incidents: store.clone(),
            uow: Arc::clone(&world.crashing) as Arc<dyn UnitOfWork>,
            effects: store.clone(),
            effect_read: store,
            clock: world.clock.clone(),
        };
        if drain_legacy_actions(&deps).await.is_err() {
            assert!(
                world.crashing.crashed(),
                "the drain failed before any crash"
            );
        }
    })
}

/// Approves `action` as a person, ten seconds after it was proposed.
fn approve(action: &mut ActionRequest) {
    let binding = PlanBinding::Full(action.plan_hash_for_approval());
    action
        .approve(
            Authorization::human(human(UserId::new())),
            ApprovalProvenance::new(None, "test"),
            &binding,
            at(10),
        )
        .expect("a proposed action is approved");
}
