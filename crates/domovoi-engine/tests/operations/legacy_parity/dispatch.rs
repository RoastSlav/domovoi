//! Parity tests: dispatch.
//! The behaviours the first-generation action executor protected, proven on the operation path.

use std::sync::Arc;

use domovoi_core::capability::Availability;
use domovoi_core::ports::{
    ActionAnswer, AggregateRef, AuditLog, EffectDraft, EffectKind, EffectStatus, HostRepo,
    LinkError, Page, Presence, SkillOutput, Transaction,
};
use domovoi_core::{
    Actor, AgentIdentity, ContractId, EffectId, ExecutionId, HostId, OperationId, OperationStatus,
    ParamKind, ParamName, ParamSpec, Risk, StepId, StepKind, StepOutcome,
};
use domovoi_testkit::build::{T0, manifest};
use serde_json::json;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::drive::{failed_output, new_epoch, ok_output};
use crate::harness::{World, host_id, restart, restart_skill, seed, world, world_gated};

/// The reason stored on the Execution step.
async fn reason(w: &World, op: OperationId, step: StepId) -> String {
    w.load(op)
        .await
        .step(step)
        .unwrap()
        .progress
        .reason
        .clone()
        .unwrap_or_default()
}

/// The output stored on the Execution step.
async fn output(w: &World, op: OperationId, step: StepId) -> String {
    w.load(op)
        .await
        .step(step)
        .unwrap()
        .progress
        .output
        .clone()
        .unwrap_or_default()
}

/// The reason the hub stores for a run it proved it never handed over.
const HANDOFF_FAILED: &str = "hub_not_handed_off";

/// How many effects of `kind` for `op` are neither done nor dead.
async fn open_effects(w: &World, op: OperationId, kind: &str) -> usize {
    let mut count = 0;
    for status in [EffectStatus::Pending, EffectStatus::Claimed] {
        count += w
            .effects(op, status)
            .await
            .iter()
            .filter(|effect| effect.kind == kind)
            .count();
    }
    count
}

/// Lets the service work and the clock run for a while.
async fn run_on(w: &World, seconds: i64) {
    for _ in 0..seconds {
        w.settle().await;
        w.advance(1);
    }
    w.settle().await;
}

/// The skill with its container pattern changed, so `plex` no longer fits it.
fn stricter_skill() -> domovoi_core::SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).unwrap();
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[A-Z]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// Proposes and approves a step, then waits until it is dispatched and held in the hand-off.
async fn held_in_handoff(w: &World, gate: &Arc<Semaphore>) -> (OperationId, StepId) {
    w.link.hold_skill_handoffs(gate.clone());
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_now(op, |s| {
        s.step(step)
            .is_some_and(|r| r.progress.outcome == StepOutcome::Running)
    })
    .await;
    (op, step)
}

#[tokio::test]
async fn args_no_longer_valid_refuse_the_step() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    // The agent now offers the skill with a stricter parameter, so the stored args fail it.
    w.link.set_skills(host_id(), vec![stricter_skill()]);
    w.queue.open();

    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert!(
        reason(&w, op, step)
            .await
            .starts_with("the arguments are no longer valid"),
        "the refusal names the arguments"
    );
    assert!(w.skill_requests().is_empty(), "nothing reached the agent");
}

#[tokio::test]
async fn skill_missing_after_approval_is_unavailable() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    // The registry still lists the skill as available, but the live session no longer offers it.
    w.link.set_skills(host_id(), Vec::new());
    w.registry.set_availability(
        host_id(),
        &ContractId::parse("skill.restart_container").unwrap(),
        Availability::Available,
    );
    w.queue.open();

    w.until_outcome(op, step, StepOutcome::Unavailable).await;

    assert_eq!(
        reason(&w, op, step).await,
        "the agent does not offer this skill"
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn skill_dropped_from_the_registry_after_approval_is_unavailable() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.link.set_skills(host_id(), Vec::new());
    w.queue.open();

    w.until_outcome(op, step, StepOutcome::Unavailable).await;

    assert!(
        !reason(&w, op, step).await.is_empty(),
        "the unavailable skill is explained"
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn step_output_is_redacted_and_capped() {
    let w = world().await;
    let noisy = format!("token=hunter2\n{}", "a line of output\n".repeat(2_000));
    let long_stderr = format!("boom hunter2 {}", "é".repeat(2_000));
    w.link.on_skill(move |_, _| {
        Ok(SkillOutput {
            stderr: long_stderr.clone(),
            ..ok_output(&noisy)
        })
    });
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let stored = snapshot
        .step(step)
        .unwrap()
        .progress
        .output
        .clone()
        .unwrap();
    assert!(!stored.contains("hunter2"), "the secret is redacted");
    assert!(stored.starts_with("token="), "the output is kept");
    assert!(stored.len() <= 16 * 1024, "the output is capped");
    assert!(
        stored.len() > 16 * 1024 - 8,
        "the cap cut the output, not an earlier step"
    );
    // The stdout alone is longer than the cap, so the stderr section was cut off.
    assert!(!stored.contains("--- stderr ---"));
}

#[tokio::test]
async fn step_output_merges_stderr_after_stdout() {
    let w = world().await;
    w.link.on_skill(|_, _| {
        Ok(SkillOutput {
            stderr: "warning: slow disk, password hunter2".to_owned(),
            ..ok_output("restarted plex")
        })
    });
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let stored = output(&w, op, step).await;
    assert!(stored.starts_with("restarted plex\n--- stderr ---\nwarning: slow disk"));
    assert!(!stored.contains("hunter2"));
}

#[tokio::test]
async fn step_output_is_cut_on_a_character_boundary() {
    let w = world().await;
    // Two-byte characters, so the 16 KiB cap may fall inside one only if the cut is careless.
    let text = format!("a{}", "é".repeat(20_000));
    w.link.on_skill(move |_, _| Ok(ok_output(&text)));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let stored = output(&w, op, step).await;
    assert!(stored.len() <= 16 * 1024);
    assert!(stored.len() >= 16 * 1024 - 1, "the cut keeps what fits");
    assert!(
        stored.chars().skip(1).all(|c| c == 'é'),
        "no split character"
    );
}

/// Every audit event, oldest first.
async fn audit_all(w: &World) -> Vec<domovoi_core::ports::AuditEvent> {
    let page = AuditLog::list(&*w.store, &Default::default(), &Page::first(500))
        .await
        .unwrap();
    let mut items = page.items;
    items.reverse();
    items
}

/// The audit kinds that belong to the execution path, oldest first.
async fn execution_kinds(w: &World) -> Vec<String> {
    audit_all(w)
        .await
        .into_iter()
        .map(|event| event.kind)
        .filter(|kind| kind.starts_with("execution."))
        .collect()
}

/// The `to` state of every execution transition journalled for `op`, oldest first.
async fn transitions(w: &World, op: OperationId) -> Vec<String> {
    w.journal(op)
        .await
        .into_iter()
        .filter(|entry| entry.kind == "operation.execution")
        .filter_map(|entry| entry.payload["to"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn step_audit_trail_is_complete_and_ordered() {
    let w = world().await;
    w.verify_after_ten_seconds();
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    w.until_verified(op).await;

    assert_eq!(
        execution_kinds(&w).await,
        [
            "execution.proposed",
            "execution.approved",
            "execution.dispatched",
            "execution.succeeded",
        ]
    );
    let entries = w.journal(op).await;
    assert!(
        entries.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "the journal sequence only grows"
    );
    assert_eq!(
        transitions(&w, op).await,
        ["awaiting_approval", "authorized", "dispatched", "succeeded"]
    );
    // The detail names the operation and step, never what the skill printed.
    for event in audit_all(&w).await {
        let detail = event.detail.to_string();
        assert!(
            !detail.contains("restarted plex"),
            "{}: {detail}",
            event.kind
        );
    }
}

#[tokio::test]
async fn step_audit_trail_of_a_failed_run_is_ordered() {
    let w = world().await;
    w.link.on_skill(|_, _| Ok(failed_output(2)));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Failed).await;

    assert_eq!(
        execution_kinds(&w).await,
        [
            "execution.proposed",
            "execution.approved",
            "execution.dispatched",
            "execution.failed",
        ]
    );
    assert_eq!(
        transitions(&w, op).await,
        ["awaiting_approval", "authorized", "dispatched", "failed"]
    );
    let entries = w.journal(op).await;
    assert!(entries.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    for event in audit_all(&w).await {
        assert!(!event.detail.to_string().contains("no such container"));
    }
}

#[tokio::test]
async fn step_audit_trail_of_a_refused_run_is_ordered() {
    let w = world().await;
    w.registry.disable(
        host_id(),
        domovoi_core::SkillName::parse("restart_container").unwrap(),
    );
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        execution_kinds(&w).await,
        [
            "execution.proposed",
            "execution.approved",
            "execution.refused"
        ]
    );
    let entries = w.journal(op).await;
    assert!(entries.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    assert_eq!(
        transitions(&w, op).await,
        ["awaiting_approval", "authorized", "refused"]
    );
}

#[tokio::test]
async fn a_run_is_not_sent_if_the_session_changed_after_the_dispatch_commit() {
    let w = world().await;
    let gate = Arc::new(Semaphore::new(0));
    let (op, step) = held_in_handoff(&w, &gate).await;

    // The agent reconnected while the dispatch was committed but not yet sent.
    w.link.new_session(host_id());
    gate.add_permits(1);

    w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&w, op, step).await, HANDOFF_FAILED);
    assert!(w.skill_requests().is_empty(), "nothing reached the agent");
}

#[tokio::test]
async fn a_run_is_not_sent_if_the_epoch_changed_after_the_dispatch_commit() {
    let w = world().await;
    let gate = Arc::new(Semaphore::new(0));
    let (op, step) = held_in_handoff(&w, &gate).await;

    // The agent's journal was replaced while the dispatch was committed but not yet sent.
    w.link.set_journal_epoch(host_id(), new_epoch());
    gate.add_permits(1);

    w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&w, op, step).await, HANDOFF_FAILED);
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn a_cancelled_step_is_not_sent_even_if_its_dispatch_effect_runs_later() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.handle()
        .cancel(op, Actor::User(w.user), "changed my mind".to_owned())
        .await
        .unwrap();

    w.queue.open();
    run_on(&w, 60).await;

    assert_eq!(w.outcome(op, step).await, StepOutcome::Cancelled);
    assert!(w.skill_requests().is_empty(), "a cancelled step never runs");
}

/// The second host the tests that need two use.
fn host_b() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002))
}

/// Enrols and connects a second host that offers the same skill.
async fn second_host(w: &World) {
    let mut registered = domovoi_testkit::build::host("backup").unwrap();
    registered.id = host_b();
    HostRepo::upsert_host(&*w.store, &registered).await.unwrap();
    HostRepo::put_agent(
        &*w.store,
        &AgentIdentity {
            agent: w.link.agent_id(host_b()),
            host: host_b(),
            public_key: [7; 32],
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
    w.link.set_presence(
        host_b(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    w.link.set_skills(host_b(), vec![restart_skill()]);
    w.link.set_journal_epoch(host_b(), new_epoch());
}

#[tokio::test]
async fn a_waiting_run_does_not_starve_another_host() {
    let w = world().await;
    second_host(&w).await;
    let waiting_op = w.create().await;
    let waiting = w.propose(waiting_op).await;
    // Host A drops off the link before its approval arrives.
    w.link.set_presence(
        host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    w.approve(waiting_op, waiting).await.unwrap();
    w.until_now(waiting_op, |s| {
        s.operation.status == OperationStatus::WaitingAgent
    })
    .await;

    let mut other_seed = seed();
    other_seed.pinned[0].host = host_b();
    let other_op = w.handle().create(other_seed).await.unwrap();
    let mut request = restart("plex");
    request.intent.host = host_b();
    let other = w
        .handle()
        .propose_execution(other_op, request)
        .await
        .unwrap();
    w.approve(other_op, other).await.unwrap();

    w.until_outcome(other_op, other, StepOutcome::Succeeded)
        .await;

    assert_eq!(w.outcome(waiting_op, waiting).await, StepOutcome::Pending);
    let sent = w.skill_requests();
    assert_eq!(sent.len(), 1, "only the host that is there got a request");
    assert_eq!(sent[0].action, ExecutionId::from_step(other).as_action());
}

#[tokio::test]
async fn approvals_caught_up_at_once_share_one_loop_guard() {
    let w = world_gated().await;
    let op = w.create().await;
    let mut steps = Vec::new();
    for _ in 0..4 {
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        steps.push(step);
    }

    w.queue.open();
    w.until(op, |s| {
        steps
            .iter()
            .all(|step| s.step(*step).is_some_and(|r| r.progress.outcome.is_final()))
    })
    .await;

    let mut refused = 0;
    for step in &steps {
        match w.outcome(op, *step).await {
            StepOutcome::Refused => {
                refused += 1;
                assert!(reason(&w, op, *step).await.starts_with("loop guard:"));
            }
            other => assert_eq!(other, StepOutcome::Succeeded),
        }
    }
    assert_eq!(refused, 1, "exactly one of four is stopped");
    assert_eq!(w.skill_requests().len(), 3, "three reached the agent");
}

#[tokio::test]
async fn a_sent_run_is_never_mistaken_for_not_run() {
    let w = world().await;
    let gate = Arc::new(Semaphore::new(0));
    let (op, step) = held_in_handoff(&w, &gate).await;

    // A reconcile pass asks the journal about the run while the hub is still handing it over.
    let now = w.now();
    w.uow
        .commit(Transaction::new().effect(EffectDraft {
            id: EffectId::new(),
            kind: EffectKind::Reconcile,
            aggregate: AggregateRef::operation(op),
            payload: json!({ "v": 1, "operation": op.to_string(), "step": step.to_string() }),
            not_before: now,
            max_attempts: 5,
            dedup_key: None,
            created_at: now,
        }))
        .await
        .unwrap();
    w.link.answer(
        host_id(),
        ExecutionId::from_step(step).as_action(),
        ActionAnswer::NotFound,
    );
    run_on(&w, 5).await;

    assert_eq!(
        w.outcome(op, step).await,
        StepOutcome::Running,
        "the run is not settled as not run while the hand-off is open"
    );
    assert!(
        w.link
            .requests()
            .iter()
            .all(|request| !matches!(request, domovoi_testkit::link::LinkRequest::Status { .. })),
        "the journal was not asked before the hand-off finished"
    );

    gate.add_permits(1);
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    assert_eq!(w.skill_requests().len(), 1);
}

#[tokio::test]
async fn an_agent_rejection_is_a_confirmed_failure() {
    let w = world().await;
    w.link.on_skill(|_, _| {
        Err(LinkError::Rejected(
            "the plan hash does not match".to_owned(),
        ))
    });
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    w.until_outcome(op, step, StepOutcome::Failed).await;
    run_on(&w, 120).await;

    assert_eq!(reason(&w, op, step).await, "the plan hash does not match");
    assert_eq!(open_effects(&w, op, "reconcile").await, 0);
    assert_eq!(w.outcome(op, step).await, StepOutcome::Failed);
    assert_eq!(w.skill_requests().len(), 1, "it was not sent again");
}

/// The causes a hand-off can fail for before the frame is queued.
fn handoff_failures() -> Vec<(&'static str, LinkError)> {
    vec![
        (
            "queue full",
            LinkError::Transport("the agent's outgoing queue is full".to_owned()),
        ),
        ("no live session", LinkError::NotConnected),
        ("timeout before send", LinkError::Timeout),
        (
            "unencodable",
            LinkError::Transport("the request could not be encoded".to_owned()),
        ),
    ]
}

#[tokio::test]
async fn a_failed_hand_off_proves_the_run_was_not_sent() {
    for (cause, error) in handoff_failures() {
        let w = world().await;
        w.link.fail_next_skill_handoff(error);
        let op = w.create().await;
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();

        w.until_outcome(op, step, StepOutcome::NotRun).await;
        run_on(&w, 120).await;

        assert_eq!(reason(&w, op, step).await, HANDOFF_FAILED, "{cause}");
        assert!(w.skill_requests().is_empty(), "{cause}: nothing was sent");
        assert_eq!(open_effects(&w, op, "reconcile").await, 0, "{cause}");
        assert_eq!(w.outcome(op, step).await, StepOutcome::NotRun, "{cause}");
    }
}

#[tokio::test]
async fn a_failed_hand_off_still_counts_toward_the_loop_guard_after_a_restart() {
    let mut w = world().await;
    let op = w.create().await;
    for (cause, error) in handoff_failures().into_iter().take(3) {
        w.link.fail_next_skill_handoff(error);
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::NotRun).await;
        assert_eq!(reason(&w, op, step).await, HANDOFF_FAILED, "{cause}");
    }

    w.restart().await;
    let fourth = w.propose(op).await;
    w.approve(op, fourth).await.unwrap();
    w.until_outcome(op, fourth, StepOutcome::Refused).await;

    assert!(reason(&w, op, fourth).await.starts_with("loop guard:"));
    assert!(
        w.skill_requests().is_empty(),
        "no run ever reached the agent"
    );
}

#[tokio::test]
async fn a_failed_hand_off_never_overwrites_a_terminal_decision() {
    // A step that was running can only be ended by its own result or by the journal, and
    // both go through the gate this hand-off holds, so a terminal outcome cannot appear
    // while the hand-off is open. The closest reachable order is the reverse: the hand-off
    // fails first, and a later answer from the journal must not rewrite the settled step.
    let w = world().await;
    w.link
        .fail_next_skill_handoff(LinkError::Transport("the agent's queue is full".to_owned()));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::NotRun).await;

    // The journal later claims the run finished; the settled step keeps its outcome.
    w.link.answer(
        host_id(),
        ExecutionId::from_step(step).as_action(),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    let now = w.now();
    w.uow
        .commit(Transaction::new().effect(EffectDraft {
            id: EffectId::new(),
            kind: EffectKind::Reconcile,
            aggregate: AggregateRef::operation(op),
            payload: json!({ "v": 1, "operation": op.to_string(), "step": step.to_string() }),
            not_before: now,
            max_attempts: 5,
            dedup_key: None,
            created_at: now,
        }))
        .await
        .unwrap();
    run_on(&w, 60).await;

    assert_eq!(w.outcome(op, step).await, StepOutcome::NotRun);
    assert_eq!(reason(&w, op, step).await, HANDOFF_FAILED);
}

#[tokio::test]
async fn rejected_or_failed_steps_never_retry_silently() {
    // Failed.
    let w = world().await;
    w.link.on_skill(|_, _| Ok(failed_output(1)));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Failed).await;
    run_on(&w, 3_700).await;
    assert_eq!(w.skill_requests().len(), 1, "a failed run is not retried");
    assert_eq!(open_effects(&w, op, "dispatch").await, 0);
    assert_eq!(w.outcome(op, step).await, StepOutcome::Failed);

    // Refused.
    let w = world().await;
    w.registry.disable(
        host_id(),
        domovoi_core::SkillName::parse("restart_container").unwrap(),
    );
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Refused).await;
    // Nothing sends the refused step again, however long the service runs.
    run_on(&w, 3_700).await;
    assert!(
        w.skill_requests().is_empty(),
        "a refused run is not retried"
    );
    assert_eq!(open_effects(&w, op, "dispatch").await, 0);
    assert_eq!(w.outcome(op, step).await, StepOutcome::Refused);

    // Only an explicit retry proposal creates a new dispatch.
    let again = w.propose(op).await;
    assert_ne!(again, step);
    let kinds: Vec<_> = w
        .load(op)
        .await
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::Execution)
        .map(|record| record.step.id)
        .collect();
    assert_eq!(kinds.len(), 2);
}
