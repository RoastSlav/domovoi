//! Parity tests: reconcile.
//! The behaviours the first-generation action executor protected, proven on the operation path.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, EffectDraft, EffectKind, EffectStatus,
    LinkError, Transaction, UnknownReason,
};
use domovoi_core::{
    ActionId, EffectId, ExecutionId, OperationId, OperationSnapshot, OperationStatus, PauseReason,
    Risk, SkillSummary, StepId, StepKind, StepOutcome, VerifySpec,
};
use domovoi_engine::operations::OperationSettings;
use domovoi_testkit::link::LinkRequest;
use jiff::{SignedDuration, Timestamp};

use crate::drive::{firing, new_epoch, ok_output};
use crate::harness::{World, host_id, restart_skill, world, world_with};

/// A run that went out and whose answer never came back, so it stays dispatched.
async fn dispatched_without_an_answer(w: &World) -> (OperationId, StepId) {
    w.link
        .on_skill(|_, _| Err(LinkError::Indeterminate("link dropped".to_owned())));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Running).await;
    (op, step)
}

/// What the journal says about `step`'s run.
fn answer(w: &World, step: StepId, answer: ActionAnswer) {
    w.link
        .answer(host_id(), ExecutionId::from_step(step).as_action(), answer);
}

/// The deadlines of every status query so far.
fn status_deadlines(w: &World) -> Vec<Timestamp> {
    w.link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Status { deadline, .. } => Some(deadline),
            _ => None,
        })
        .collect()
}

/// The deadlines of every re-check so far.
fn check_deadlines(w: &World) -> Vec<Timestamp> {
    w.link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Check { deadline, .. } => Some(deadline),
            _ => None,
        })
        .collect()
}

/// The reason stored on `step`.
fn reason_of(snapshot: &OperationSnapshot, step: StepId) -> String {
    snapshot
        .step(step)
        .unwrap()
        .progress
        .reason
        .clone()
        .unwrap_or_default()
}

/// Whether `snapshot` holds a Verification step.
fn has_verification(snapshot: &OperationSnapshot) -> bool {
    snapshot
        .steps
        .iter()
        .any(|record| record.step.kind == StepKind::Verification)
}

/// Every pending effect of `kind` on `op`.
async fn pending(w: &World, op: OperationId, kind: &str) -> Vec<domovoi_core::ports::EffectRecord> {
    w.effects(op, EffectStatus::Pending)
        .await
        .into_iter()
        .filter(|effect| effect.kind == kind)
        .collect()
}

/// How the fake agent answers the hub's status queries.
#[derive(Clone, Copy)]
enum Status {
    /// The run is still going.
    Running,
    /// The agent recognised nothing it was asked about.
    Silent,
    /// The query itself times out.
    TimedOut,
}

fn script_status(w: &World, status: Status) {
    match status {
        Status::Running => script_running(w),
        Status::Silent => w.link.on_action_status(|_, _, epoch| {
            Ok(ActionAnswers {
                epoch,
                answers: Vec::new(),
            })
        }),
        Status::TimedOut => w.link.on_action_status(|_, _, _| Err(LinkError::Timeout)),
    }
}

/// A status handler that answers `Running` for whatever it is asked.
fn script_running(w: &World) {
    w.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids
                .iter()
                .map(|id| ActionAnswerEntry {
                    execution: *id,
                    answer: ActionAnswer::Running,
                })
                .collect(),
        })
    });
}

/// One scenario of test 1: the hand-over fails with `dispatch`, the journal answers `status`.
async fn stays_dispatched(dispatch: LinkError, status: Status) {
    let mut w = world().await;
    script_status(&w, status);
    w.link.on_skill(move |_, _| Err(dispatch.clone()));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Running).await;
    w.until(op, |_| !status_deadlines(&w).is_empty()).await;

    w.restart().await;
    let before = status_deadlines(&w).len();
    w.until(op, |_| status_deadlines(&w).len() > before).await;
    w.settle().await;

    assert_eq!(w.outcome(op, step).await, StepOutcome::Running);
    assert_eq!(w.skill_requests().len(), 1, "the run was never sent again");
    assert_eq!(
        pending(&w, op, "reconcile").await.len(),
        1,
        "reconciliation keeps asking instead of dispatching again"
    );
    assert!(pending(&w, op, "dispatch").await.is_empty());
}

#[tokio::test]
async fn dispatched_step_stays_dispatched_after_a_restart() {
    // The agent says the run is still going.
    stays_dispatched(
        LinkError::Indeterminate("link dropped".to_owned()),
        Status::Running,
    )
    .await;
    // The agent answers about nothing it was asked.
    stays_dispatched(
        LinkError::Indeterminate("link dropped".to_owned()),
        Status::Silent,
    )
    .await;
    // The result wait timed out, and the journal cannot be reached either.
    stays_dispatched(LinkError::Timeout, Status::TimedOut).await;
    // The result wait timed out; the journal says it is still going.
    stays_dispatched(LinkError::Timeout, Status::Running).await;
}

#[tokio::test]
async fn loop_guard_survives_a_restart() {
    let mut w = world().await;
    let op = w.create().await;
    for _ in 0..3 {
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::Succeeded).await;
    }

    w.restart().await;
    let fourth = w.propose(op).await;
    w.approve(op, fourth).await.unwrap();
    let snapshot = w.until_outcome(op, fourth, StepOutcome::Refused).await;

    assert!(reason_of(&snapshot, fourth).starts_with("loop guard:"));
    assert_eq!(w.skill_requests().len(), 3, "the fourth was never sent");
}

#[tokio::test]
async fn unknown_answers_settle_outcome_unknown() {
    let mut texts = Vec::new();
    for reason in [
        UnknownReason::NoStartRecord,
        UnknownReason::AgentRestarted,
        UnknownReason::JournalLost,
        UnknownReason::EpochUnknown,
    ] {
        let mut w = world().await;
        let (op, step) = dispatched_without_an_answer(&w).await;
        answer(&w, step, ActionAnswer::Unknown { reason });

        w.restart().await;
        let snapshot = w.until_outcome(op, step, StepOutcome::OutcomeUnknown).await;

        assert_eq!(
            snapshot.operation.status,
            OperationStatus::Paused {
                reason: PauseReason::OutcomeUnknown { step }
            }
        );
        texts.push(reason_of(&snapshot, step));
        w.advance(3600);
        w.settle().await;
        assert_eq!(w.skill_requests().len(), 1, "nothing was sent again");
        assert_eq!(w.outcome(op, step).await, StepOutcome::OutcomeUnknown);
    }
    assert!(texts.iter().all(|text| !text.is_empty()));
    let distinct: std::collections::BTreeSet<_> = texts.iter().collect();
    assert_eq!(distinct.len(), texts.len(), "each reason has its own text");
}

/// Hub side of the crash window where the agent journal accepted a run but never recorded
/// its start: the answer settles `OutcomeUnknown`, audited once, with no success anywhere.
#[tokio::test]
async fn crash_after_accepted_is_outcome_unknown() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    answer(
        &w,
        step,
        ActionAnswer::Unknown {
            reason: UnknownReason::NoStartRecord,
        },
    );

    w.restart().await;
    let snapshot = w.until_outcome(op, step, StepOutcome::OutcomeUnknown).await;
    w.settle().await;

    assert_eq!(
        snapshot.operation.status,
        OperationStatus::Paused {
            reason: PauseReason::OutcomeUnknown { step }
        }
    );
    assert_eq!(
        reason_of(&snapshot, step),
        "the agent recorded this run but not its start; it may not have run"
    );
    let audit = w.audit_kinds().await;
    assert_eq!(
        audit
            .iter()
            .filter(|kind| kind.as_str() == "execution.outcome_unknown")
            .count(),
        1,
        "{audit:?}"
    );
    assert!(
        audit.iter().all(|kind| !kind.contains("succeeded")),
        "{audit:?}"
    );
    let event = w.audit_event("execution.outcome_unknown").await.unwrap();
    assert!(!event.detail.to_string().to_lowercase().contains("succe"));
    assert!(!reason_of(&snapshot, step).to_lowercase().contains("succe"));
    assert!(!has_verification(&snapshot), "no verification was queued");
}

#[tokio::test]
async fn a_stale_answer_from_another_epoch_settles_outcome_unknown() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    // The agent answers under a journal that is not the one the run was dispatched under, and
    // what it says looks like a success.
    let stale = new_epoch();
    w.link.on_action_status(move |_, ids, _| {
        Ok(ActionAnswers {
            epoch: stale,
            answers: ids
                .iter()
                .map(|id| ActionAnswerEntry {
                    execution: *id,
                    answer: ActionAnswer::Finished {
                        succeeded: true,
                        detail: None,
                    },
                })
                .collect(),
        })
    });

    w.restart().await;
    let snapshot = w.until_outcome(op, step, StepOutcome::OutcomeUnknown).await;

    assert!(reason_of(&snapshot, step).contains("journal was reset"));
    assert!(matches!(
        snapshot.operation.status,
        OperationStatus::Paused { .. }
    ));
    assert_eq!(w.skill_requests().len(), 1);
    assert!(!has_verification(&snapshot));
}

#[tokio::test]
async fn a_failed_finish_keeps_the_agents_message() {
    for (detail, expected) in [
        (Some("disk is full".to_owned()), "disk is full"),
        (None, "the run failed"),
    ] {
        let mut w = world().await;
        w.verify_after_ten_seconds();
        let (op, step) = dispatched_without_an_answer(&w).await;
        answer(
            &w,
            step,
            ActionAnswer::Finished {
                succeeded: false,
                detail,
            },
        );

        w.restart().await;
        let snapshot = w.until_outcome(op, step, StepOutcome::Failed).await;

        assert_eq!(reason_of(&snapshot, step), expected);
        assert!(!has_verification(&snapshot), "a failed run is not verified");
        assert_eq!(w.skill_requests().len(), 1);
    }
}

#[tokio::test]
async fn reconciliation_is_idempotent() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    answer(
        &w,
        step,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    w.restart().await;
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    w.settle().await;
    let settled = w.load(op).await;
    let succeeded_events = |kinds: Vec<String>| {
        kinds
            .iter()
            .filter(|kind| *kind == "execution.succeeded")
            .count()
    };
    let events = succeeded_events(w.audit_kinds().await);

    // A second reconcile for the same step, as a duplicate in the outbox would be.
    let now = w.now();
    let again = EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Reconcile,
        aggregate: domovoi_core::ports::AggregateRef::operation(op),
        payload: serde_json::json!({
            "v": 1, "operation": op.to_string(), "step": step.to_string(),
        }),
        not_before: now,
        max_attempts: 5,
        dedup_key: None,
        created_at: now,
    };
    w.uow
        .commit(Transaction::new().effect(again))
        .await
        .unwrap();
    for _ in 0..20 {
        w.settle().await;
        w.advance(1);
    }

    let after = w.load(op).await;
    assert_eq!(after.version, settled.version);
    assert_eq!(after.operation.status, settled.operation.status);
    assert_eq!(after.steps.len(), settled.steps.len());
    assert_eq!(w.outcome(op, step).await, StepOutcome::Succeeded);
    assert_eq!(succeeded_events(w.audit_kinds().await), events);
    assert_eq!(w.skill_requests().len(), 1);
    assert!(pending(&w, op, "reconcile").await.is_empty());
}

#[tokio::test]
async fn outcome_unknown_is_not_the_loop_guard() {
    // An unknown outcome may have happened, so it counts as one of the hour's runs; it does
    // not stop unrelated work by itself.
    let w = world().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    w.link.on_skill(move |_, _| {
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(LinkError::Indeterminate("link dropped".to_owned()))
        } else {
            Ok(ok_output("restarted plex"))
        }
    });
    let unknown_op = w.create().await;
    let unknown_step = w.propose(unknown_op).await;
    w.approve(unknown_op, unknown_step).await.unwrap();
    w.until_outcome(unknown_op, unknown_step, StepOutcome::Running)
        .await;
    answer(
        &w,
        unknown_step,
        ActionAnswer::Unknown {
            reason: UnknownReason::NoStartRecord,
        },
    );
    w.until_outcome(unknown_op, unknown_step, StepOutcome::OutcomeUnknown)
        .await;

    // One unknown run does not block the next ones.
    for _ in 0..2 {
        let op = w.create().await;
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::Succeeded).await;
    }
    // But it was a run: with two successes it fills the hour's three.
    let op = w.create().await;
    let blocked = w.propose(op).await;
    w.approve(op, blocked).await.unwrap();
    let snapshot = w.until_outcome(op, blocked, StepOutcome::Refused).await;
    assert!(reason_of(&snapshot, blocked).starts_with("loop guard:"));
    assert_eq!(w.skill_requests().len(), 3);
}

/// Reconciles a dispatched run whose journal answers with `entries` (ids are the run's own
/// or foreign ones) and returns the world, operation and step.
async fn reconcile_with(
    entries: impl Fn(ActionId) -> Vec<(ActionId, ActionAnswer)> + Send + Sync + 'static,
) -> (World, OperationId, StepId) {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    let wanted = ExecutionId::from_step(step).as_action();
    w.link.on_action_status(move |_, _, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: entries(wanted)
                .into_iter()
                .map(|(execution, answer)| ActionAnswerEntry { execution, answer })
                .collect(),
        })
    });
    w.restart().await;
    (w, op, step)
}

fn finished(succeeded: bool) -> ActionAnswer {
    ActionAnswer::Finished {
        succeeded,
        detail: Some(format!("finished {succeeded}")),
    }
}

#[tokio::test]
async fn contradictory_or_unasked_answers_settle_nothing() {
    // An answer for a run nobody asked about is ignored: the step keeps waiting.
    let foreign = ExecutionId::from_step(StepId::new()).as_action();
    let (w, op, step) = reconcile_with(move |_| vec![(foreign, finished(true))]).await;
    let before = status_deadlines(&w).len();
    w.until(op, |_| status_deadlines(&w).len() > before + 1)
        .await;
    assert_eq!(w.outcome(op, step).await, StepOutcome::Running);
    assert_eq!(w.skill_requests().len(), 1);
    drop(w);

    // Identical duplicates for the run are the same answer.
    let (w, op, step) =
        reconcile_with(|wanted| vec![(wanted, finished(true)), (wanted, finished(true))]).await;
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    drop(w);

    // An unasked answer next to the run's own does not change the run's.
    let (w, op, step) =
        reconcile_with(move |wanted| vec![(foreign, finished(false)), (wanted, finished(true))])
            .await;
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    drop(w);

    // Contradictory duplicates for the run prove nothing, in either order: the step keeps
    // running and is asked about again.
    for entries in [[false, true], [true, false]] {
        let (w, op, step) = reconcile_with(move |wanted| {
            entries
                .iter()
                .map(|succeeded| (wanted, finished(*succeeded)))
                .collect()
        })
        .await;
        let before = status_deadlines(&w).len();
        w.until(op, |_| status_deadlines(&w).len() > before + 1)
            .await;
        assert_eq!(w.outcome(op, step).await, StepOutcome::Running);
        assert_eq!(w.skill_requests().len(), 1);
    }
}

#[tokio::test]
async fn finished_failure_and_success_replies_lost_are_recovered_from_the_journal() {
    // A success whose reply was lost: the skill asks for a check, so the step is verified.
    let mut w = world().await;
    w.verify_after_ten_seconds();
    let (op, step) = dispatched_without_an_answer(&w).await;
    answer(
        &w,
        step,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    w.restart().await;
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    let snapshot = w.until_verified(op).await;
    assert!(has_verification(&snapshot));
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(w.skill_requests().len(), 1);

    // A success of a skill with no verify and no applicable check owes no verification.
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    answer(
        &w,
        step,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    w.restart().await;
    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;
    assert!(!has_verification(&snapshot));

    // A failure whose reply was lost is recovered as a failure, with the agent's message.
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    answer(
        &w,
        step,
        ActionAnswer::Finished {
            succeeded: false,
            detail: Some("exit code 3".to_owned()),
        },
    );
    w.restart().await;
    let snapshot = w.until_outcome(op, step, StepOutcome::Failed).await;
    assert_eq!(reason_of(&snapshot, step), "exit code 3");
}

/// The skill a case offers: `verify`, `risk`, and whether it applies to the execution's check.
fn skill_for(verify: Option<VerifySpec>, risk: Risk, applies: bool) -> SkillSummary {
    let mut skill = restart_skill();
    skill.verify = verify;
    skill.risk = risk;
    if applies {
        skill.applies_to = vec![crate::harness::subject().check];
    }
    skill
}

/// The delay between a run's success and its re-check, as the verify effect carries it, or
/// `None` when the run owes no verification. `skill` is what the agent offers when the run is
/// settled; `reconciled` settles it through the journal rather than the live answer.
async fn verification_delay(
    skill: Option<SkillSummary>,
    reconciled: bool,
) -> Option<SignedDuration> {
    let mut w = world().await;
    let op;
    let step;
    if reconciled {
        (op, step) = dispatched_without_an_answer(&w).await;
        w.link
            .set_skills(host_id(), skill.clone().into_iter().collect());
        answer(
            &w,
            step,
            ActionAnswer::Finished {
                succeeded: true,
                detail: None,
            },
        );
        w.restart().await;
    } else {
        w.link
            .set_skills(host_id(), skill.clone().into_iter().collect());
        op = w.create().await;
        step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
    }
    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;
    w.settle().await;
    let settled = snapshot
        .execution_of(step)
        .unwrap()
        .value
        .settled_at
        .unwrap();
    let verify = pending(&w, op, "verify").await;
    let delay = verify
        .first()
        .map(|effect| effect.not_before.duration_since(settled));
    assert_eq!(
        has_verification(&w.load(op).await),
        delay.is_some(),
        "a Verification step exists exactly when a re-check is queued"
    );
    delay
}

#[tokio::test]
async fn verification_delay_follows_the_skills_verify_spec_or_the_write_default() {
    let default = OperationSettings::default().verify_default_delay;
    let recheck = |recheck, secs| {
        Some(VerifySpec {
            recheck,
            delay: SignedDuration::from_secs(secs),
        })
    };
    let cases: Vec<(&str, Option<SkillSummary>, Option<SignedDuration>)> = vec![
        (
            "a verify spec with recheck wins over the default",
            Some(skill_for(recheck(true, 90), Risk::Write, false)),
            Some(SignedDuration::from_secs(90)),
        ),
        (
            "recheck = false means no verification, even for an applicable write skill",
            Some(skill_for(recheck(false, 90), Risk::Write, true)),
            None,
        ),
        (
            "a write skill that applies to the check waits the default",
            Some(skill_for(None, Risk::Write, true)),
            Some(default),
        ),
        (
            "a write skill that does not apply is not verified",
            Some(skill_for(None, Risk::Write, false)),
            None,
        ),
        (
            "a read skill is not verified",
            Some(skill_for(None, Risk::Read, true)),
            None,
        ),
    ];
    for (name, skill, expected) in cases {
        for reconciled in [false, true] {
            let got = verification_delay(skill.clone(), reconciled).await;
            assert_eq!(got, expected, "{name} (reconciled: {reconciled})");
        }
    }
    // The agent no longer offers the skill when the run is settled from the journal.
    assert_eq!(verification_delay(None, true).await, None, "no skill");
}

#[tokio::test]
async fn verification_recheck_uses_a_sixty_second_deadline() {
    assert_eq!(
        OperationSettings::default().recheck_deadline,
        SignedDuration::from_secs(60)
    );

    // The re-check after a success.
    let w = world().await;
    w.verify_after_ten_seconds();
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    w.advance(11);
    w.until(op, |_| !check_deadlines(&w).is_empty()).await;
    let asked = check_deadlines(&w);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0], w.now() + SignedDuration::from_secs(60));

    // The status query that reconciles a dispatched run.
    let mut w = world().await;
    let (op, _) = dispatched_without_an_answer(&w).await;
    script_running(&w);
    w.restart().await;
    w.until(op, |_| !status_deadlines(&w).is_empty()).await;
    let first = status_deadlines(&w)[0];
    assert_eq!(first, w.now() + SignedDuration::from_secs(60));
}

#[tokio::test]
async fn failed_verification_starts_another_cycle_or_reaches_a_human() {
    let mut settings = OperationSettings::default();
    settings.budgets.cycles = Some(2);
    settings.loop_guard_per_hour = 10;
    let w = world_with(settings).await;
    w.verify_after_ten_seconds();
    w.link.on_check(|_, _, _| Ok(firing()));
    let op = w.create().await;

    for cycle in 0..2u32 {
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::Succeeded).await;
        let wanted = usize::try_from(cycle).unwrap() + 1;
        let snapshot = w.until(op, |s| failed_verifications(s) >= wanted).await;
        assert_eq!(snapshot.operation.cycle, cycle + 1, "a new cycle begins");
        assert!(!snapshot.operation.status.is_terminal());
        let key = format!("build:{op}:{}", cycle + 1);
        let asked = pending(&w, op, "build_context").await;
        assert_eq!(
            asked
                .iter()
                .filter(|effect| effect.dedup_key.as_deref() == Some(key.as_str()))
                .count(),
            1,
            "the new cycle asks for its context once"
        );
        assert_eq!(
            snapshot.step(step).unwrap().progress.outcome,
            StepOutcome::Succeeded
        );
        // Let the next cycle's context request be answered before the next proposal.
        w.settle().await;
    }

    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    let snapshot = w
        .until(op, |s| s.operation.status == OperationStatus::Escalated)
        .await;
    assert_eq!(failed_verifications(&snapshot), 3);
    assert_eq!(snapshot.operation.cycle, 2, "no further cycle starts");
}

/// How many Verification steps ended failed.
fn failed_verifications(snapshot: &OperationSnapshot) -> usize {
    snapshot
        .steps
        .iter()
        .filter(|record| {
            record.step.kind == StepKind::Verification
                && record.progress.outcome == StepOutcome::Failed
        })
        .count()
}
