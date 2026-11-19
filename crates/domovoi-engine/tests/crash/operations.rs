//! The operation lifecycle the crash sweep drives: creating, proposing, approving, denying,
//! asking, pausing, resuming and cancelling, so every operation write is crashed at every
//! commit.
//!
//! Like the other scenarios it is deterministic: the clock only moves when a step moves it,
//! and every step is awaited before the next one starts.

use std::collections::BTreeMap;

use domovoi_core::ports::OperationRepo;
use domovoi_core::{
    Actor, CheckKey, ContractId, ExecutionIntent, Incident, PlanBinding, StepId, StepIntent,
    StepKind, TargetKey, UserId,
};
use domovoi_engine::operations::{ExecutionRequest, NewStep};
use domovoi_testkit::approvals::{authorize, human, provenance};
use domovoi_testkit::crash::SweepStore;
use jiff::SignedDuration;

use super::sweep::BoxFuture;
use super::world::{World, host_id, operation_seed};

/// Awaits one step. After a crash the scenario stops, because nothing it does can be stored
/// any more; before one, a failed step is a test bug and stops the test.
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

/// A restart of the `nginx` container, the subject the operations are pinned to.
fn restart() -> ExecutionRequest {
    let mut args = BTreeMap::new();
    args.insert("container".to_owned(), "nginx".to_owned());
    ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse("skill.restart_container").expect("a valid contract"),
            contract_version: 1,
            host: host_id(),
            check: CheckKey::parse("docker.container").expect("a valid check"),
            target: Some(TargetKey::parse("nginx").expect("a valid target")),
            args,
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    }
}

/// An operation from its creation to its cancellation: a first evidence step, a restart
/// that is approved, a restart that is denied, a question that is paused over and answered,
/// and the cancellation.
pub fn operation_lifecycle<S: SweepStore + OperationRepo>(
    world: &mut World<S>,
) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        const NAME: &str = "operation_lifecycle";
        let user = UserId::new();
        let incident = world
            .incidents()
            .await
            .iter()
            .find(|incident| incident.check().as_str() == "docker.container")
            .map(Incident::id)
            .expect("the seeded incident is there");

        // 1. Creating an operation with a first evidence request, linked to the incident.
        let mut seed = operation_seed(vec![incident]);
        seed.first_steps.push(NewStep {
            id: StepId::new(),
            kind: StepKind::EvidenceRequest,
            phase: "evidence".to_owned(),
            parent: None,
            intent: StepIntent::EvidenceRequest {
                host: host_id(),
                tool: "docker_ps".to_owned(),
                args: serde_json::json!({}),
            },
        });
        let op = step!(world, NAME, world.operations.create(seed));

        // 2. A restart is proposed and a person approves it.
        let approved = step!(
            world,
            NAME,
            world.operations.propose_execution(op, restart())
        );
        let plan = OperationRepo::load(&*world.store, op)
            .await
            .expect("the operation is readable")
            .and_then(|snapshot| {
                snapshot
                    .execution_of(approved)
                    .map(|row| row.value.plan_hash)
            })
            .expect("the proposal stored an execution");
        step!(
            world,
            NAME,
            world.operations.authorize(
                op,
                approved,
                authorize(human(user)),
                PlanBinding::Full(plan),
                provenance(),
            )
        );

        // 3. Another restart is proposed and denied.
        let denied = step!(
            world,
            NAME,
            world.operations.propose_execution(op, restart())
        );
        step!(world, NAME, world.operations.deny(op, denied, human(user)));

        // 4. A question, a pause over it, the resume and the answer.
        let question = NewStep {
            id: StepId::new(),
            kind: StepKind::Question,
            phase: "ask".to_owned(),
            parent: None,
            intent: StepIntent::Question {
                text: "Restart nginx?".to_owned(),
                expires_at: world.now() + SignedDuration::from_hours(1),
            },
        };
        let question_id = question.id;
        step!(
            world,
            NAME,
            world.operations.propose(op, vec![question], Vec::new())
        );
        let actor = Actor::User(user);
        step!(world, NAME, world.operations.pause(op, actor.clone()));
        step!(
            world,
            NAME,
            world.operations.resume(op, actor.clone(), Vec::new())
        );
        step!(
            world,
            NAME,
            world
                .operations
                .answer(op, question_id, "yes".to_owned(), actor.clone())
        );

        // 5. The operation is cancelled.
        step!(
            world,
            NAME,
            world
                .operations
                .cancel(op, actor, "no longer needed".to_owned())
        );
    })
}
