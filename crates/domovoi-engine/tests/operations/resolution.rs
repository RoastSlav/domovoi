//! Resolution summaries: when the draft effect is created, what it writes, and how an owner
//! edits the narrative through the operation writer.

use domovoi_core::experience::{SummaryField, SummaryRevision, SummarySource};
use domovoi_core::ports::{EffectKind, EffectStatus, ExperienceRead};
use domovoi_core::{
    Actor, BudgetLimits, FinishOutcome, OperationId, OperationStatus, StepIntent, StepKind,
    StepOutcome,
};
use domovoi_engine::operations::{
    EditSummaryError, EditSummaryField, NewStep, OperationEvent, SummaryError, audit,
};

use crate::harness::{World, world, world_gated};
use crate::support::Fixture;

fn drafts(decision: &domovoi_engine::operations::Decision) -> Vec<String> {
    decision
        .effects
        .iter()
        .filter(|spec| spec.kind == EffectKind::DraftResolution)
        .map(|spec| spec.dedup_key.clone().unwrap_or_default())
        .collect()
}

#[test]
fn every_terminal_transition_creates_exactly_one_draft_effect() {
    for outcome in [
        FinishOutcome::Succeeded,
        FinishOutcome::Failed,
        FinishOutcome::Escalated,
    ] {
        let mut fx = Fixture::running(BudgetLimits::default());
        let id = fx.state.operation().id;
        let decision = fx.event(OperationEvent::Finish {
            outcome,
            summary: "done".to_owned(),
        });
        assert_eq!(drafts(&decision), vec![format!("resolution:{id}")]);
        let spec = decision
            .effects
            .iter()
            .find(|spec| spec.kind == EffectKind::DraftResolution)
            .unwrap();
        assert_eq!(
            spec.payload,
            serde_json::json!({ "v": 1, "operation": id.to_string() })
        );
    }

    let mut fx = Fixture::running(BudgetLimits::default());
    let id = fx.state.operation().id;
    let decision = fx.event(OperationEvent::Cancel {
        reason: "no longer needed".to_owned(),
    });
    assert_eq!(drafts(&decision), vec![format!("resolution:{id}")]);
}

#[test]
fn a_passing_verification_that_ends_the_operation_creates_the_draft_effect() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let exec = fx.dispatched_run();
    fx.settle(
        exec,
        StepOutcome::Succeeded,
        Some(jiff::SignedDuration::from_secs(60)),
    )
    .unwrap();
    let verification = fx.verification_of(exec);
    let decision = fx.event(OperationEvent::Verified {
        step: verification,
        passed: true,
        summary: "up".to_owned(),
    });
    assert_eq!(fx.status(), OperationStatus::Succeeded);
    assert_eq!(drafts(&decision).len(), 1);
}

#[test]
fn an_event_that_does_not_end_the_operation_creates_no_draft_effect() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let step = fx.propose_evidence();
    assert_eq!(fx.outcome(step), StepOutcome::Pending);
    let decision = fx.event(OperationEvent::Tick);
    assert!(drafts(&decision).is_empty());
}

#[test]
fn a_late_result_after_a_cancel_queues_one_more_draft_under_its_own_key() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let exec = fx.dispatched_run();
    let id = fx.state.operation().id;
    let cancelled = fx.event(OperationEvent::Cancel {
        reason: "stop".to_owned(),
    });
    assert_eq!(drafts(&cancelled), vec![format!("resolution:{id}")]);

    let settled = fx.settle(exec, StepOutcome::Succeeded, None).unwrap();
    assert_eq!(drafts(&settled), vec![format!("resolution:{id}:{exec}")]);
}

/// Waits until `op` has a summary, letting the service work.
async fn until_summary(w: &World, op: OperationId) -> Vec<SummaryRevision> {
    for _ in 0..400 {
        w.settle().await;
        let current = w.store.current(op).await.unwrap();
        if !current.is_empty() {
            return current;
        }
        w.advance(1);
    }
    panic!("the summary was never written");
}

fn value_of(revisions: &[SummaryRevision], field: SummaryField) -> Option<serde_json::Value> {
    revisions
        .iter()
        .find(|rev| rev.field == field)
        .map(|rev| rev.value.clone())
}

/// A finished operation: a run that worked and a re-check that passed.
async fn verified_fix(w: &World) -> OperationId {
    w.verify_after_ten_seconds();
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until(op, |s| s.operation.status == OperationStatus::Succeeded)
        .await;
    op
}

fn text_step(kind: StepKind, text: &str) -> NewStep {
    let intent = match kind {
        StepKind::Assessment => StepIntent::Assessment {
            text: text.to_owned(),
        },
        _ => StepIntent::Plan {
            text: text.to_owned(),
        },
    };
    NewStep {
        id: domovoi_core::StepId::new(),
        kind,
        phase: "think".to_owned(),
        parent: None,
        intent,
    }
}

#[tokio::test]
async fn a_verified_fix_gets_five_code_fields_written_by_the_service() {
    let w = world().await;
    let op = verified_fix(&w).await;

    let current = until_summary(&w, op).await;

    for field in [
        SummaryField::Outcome,
        SummaryField::Verified,
        SummaryField::Subjects,
        SummaryField::ContractsExecuted,
        SummaryField::Hosts,
    ] {
        let rev = current.iter().find(|r| r.field == field).unwrap();
        assert_eq!(rev.source, SummarySource::Code);
        assert_eq!(rev.actor, "system:operations");
        assert_eq!(rev.revision, 1);
    }
    assert_eq!(
        value_of(&current, SummaryField::Outcome),
        Some(serde_json::json!("succeeded"))
    );
    assert_eq!(
        value_of(&current, SummaryField::Verified),
        Some(serde_json::json!(true))
    );
    assert!(value_of(&current, SummaryField::Problem).is_none());
    let done = w.effects(op, EffectStatus::Done).await;
    assert!(
        done.iter()
            .any(|e| e.kind == EffectKind::DraftResolution.as_str())
    );
}

#[tokio::test]
async fn the_narrative_falls_back_to_the_assessment_and_the_plan() {
    let w = world().await;
    let op = w.create().await;
    w.handle()
        .propose(
            op,
            vec![
                text_step(
                    StepKind::Assessment,
                    "plex keeps restarting\nthe disk is full",
                ),
                text_step(StepKind::Plan, "free space, then restart plex"),
            ],
            Vec::new(),
        )
        .await
        .unwrap();
    w.handle()
        .finish(op, FinishOutcome::Failed, "gave up".to_owned())
        .await
        .unwrap();

    let current = until_summary(&w, op).await;

    let text = |field| match value_of(&current, field) {
        Some(serde_json::Value::String(text)) => text,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(text(SummaryField::Problem), "plex keeps restarting");
    assert_eq!(text(SummaryField::Cause), "the disk is full");
    assert_eq!(text(SummaryField::Fix), "free space, then restart plex");
    let problem = current
        .iter()
        .find(|r| r.field == SummaryField::Problem)
        .unwrap();
    assert_eq!(problem.source, SummarySource::Model);
}

#[tokio::test]
async fn a_summary_without_any_report_has_code_fields_only() {
    let w = world().await;
    let op = w.create().await;
    w.handle()
        .finish(op, FinishOutcome::Failed, "nothing worked".to_owned())
        .await
        .unwrap();

    let current = until_summary(&w, op).await;

    assert_eq!(current.len(), 5);
    assert!(current.iter().all(|rev| rev.source == SummarySource::Code));
}

fn edit(field: SummaryField, value: &str, expected_revision: u32) -> EditSummaryField {
    EditSummaryField {
        field,
        value: value.to_owned(),
        expected_revision,
    }
}

async fn drafted_failure(w: &World) -> OperationId {
    let op = w.create().await;
    w.handle()
        .propose(
            op,
            vec![
                text_step(StepKind::Assessment, "disk full\nthe media disk filled up"),
                text_step(StepKind::Plan, "free space"),
            ],
            Vec::new(),
        )
        .await
        .unwrap();
    w.handle()
        .finish(op, FinishOutcome::Failed, "gave up".to_owned())
        .await
        .unwrap();
    until_summary(w, op).await;
    for _ in 0..50 {
        w.settle().await;
    }
    op
}

#[tokio::test]
async fn code_owned_summary_fields_reject_user_edits() {
    let w = world().await;
    let op = drafted_failure(&w).await;
    let before = w.store.revisions(op).await.unwrap().len();
    let journal_before = w.journal(op).await.len();

    for field in [
        SummaryField::Outcome,
        SummaryField::Verified,
        SummaryField::Subjects,
        SummaryField::ContractsExecuted,
        SummaryField::Hosts,
    ] {
        let err = w
            .handle()
            .edit_summary_field(op, edit(field, "x", 1), Actor::User(w.user))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            EditSummaryError::Summary(SummaryError::CodeOwned(f)) if f == field
        ));
    }

    assert_eq!(w.store.revisions(op).await.unwrap().len(), before);
    assert_eq!(w.journal(op).await.len(), journal_before);
    assert!(
        w.audit_event(audit::OPERATION_SUMMARY_EDITED)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn an_edit_writes_a_user_revision_a_journal_row_and_an_audit_row_without_the_text() {
    let w = world().await;
    let op = drafted_failure(&w).await;

    let edited = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Cause, "the media disk filled up hunter2", 1),
            Actor::User(w.user),
        )
        .await
        .unwrap();

    assert!(!edited.unchanged);
    assert_eq!(edited.revision.revision, 2);
    assert_eq!(edited.revision.source, SummarySource::User);
    assert!(!edited.revision.value.to_string().contains("hunter2"));
    let stored = w.store.current(op).await.unwrap();
    let cause = stored
        .iter()
        .find(|r| r.field == SummaryField::Cause)
        .unwrap();
    assert_eq!(cause.revision, 2);
    assert!(!cause.value.to_string().contains("hunter2"));

    let rows: Vec<_> = w
        .journal(op)
        .await
        .into_iter()
        .filter(|e| e.kind == "operation.summary")
        .collect();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|e| !e.payload.to_string().contains("disk")));
    let last = rows.last().unwrap();
    assert_eq!(last.payload["field"], "cause");
    assert_eq!(last.payload["revision"], 2);
    assert_eq!(last.payload["source"], "user");

    let audit_row = w
        .audit_event(audit::OPERATION_SUMMARY_EDITED)
        .await
        .unwrap();
    assert!(!audit_row.detail.to_string().contains("disk"));
    assert!(!audit_row.detail.to_string().contains("hunter2"));
    assert_eq!(audit_row.detail["revision"], 2);
}

#[tokio::test]
async fn a_stale_edit_is_refused_and_changes_nothing() {
    let w = world().await;
    let op = drafted_failure(&w).await;
    let first = edit(SummaryField::Fix, "free the disk", 1);
    w.handle()
        .edit_summary_field(op, first, Actor::User(w.user))
        .await
        .unwrap();

    let err = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Fix, "something else", 1),
            Actor::User(w.user),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        EditSummaryError::Summary(SummaryError::Stale {
            expected: 1,
            actual: 2
        })
    ));
    let fix = w
        .store
        .current(op)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.field == SummaryField::Fix)
        .unwrap();
    assert_eq!(fix.revision, 2);
}

#[tokio::test]
async fn editing_a_field_with_no_revision_expects_zero() {
    let w = world().await;
    let op = w.create().await;
    w.handle()
        .finish(op, FinishOutcome::Failed, "nothing".to_owned())
        .await
        .unwrap();
    until_summary(&w, op).await;

    let err = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Problem, "a problem", 1),
            Actor::User(w.user),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        EditSummaryError::Summary(SummaryError::Stale {
            expected: 1,
            actual: 0
        })
    ));
    let ok = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Problem, "a problem", 0),
            Actor::User(w.user),
        )
        .await
        .unwrap();
    assert_eq!(ok.revision.revision, 1);
}

#[tokio::test]
async fn editing_an_open_operation_is_refused() {
    let w = world().await;
    let op = w.create().await;
    let err = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Problem, "early", 0),
            Actor::User(w.user),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        EditSummaryError::Summary(SummaryError::NotTerminal)
    ));
    assert!(w.store.revisions(op).await.unwrap().is_empty());
}

#[tokio::test]
async fn confirming_a_model_draft_appends_a_user_revision_and_repeating_it_is_a_no_op() {
    let w = world().await;
    let op = drafted_failure(&w).await;

    let confirmed = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Problem, "disk full", 1),
            Actor::User(w.user),
        )
        .await
        .unwrap();
    assert_eq!(confirmed.revision.revision, 2);
    assert_eq!(confirmed.revision.source, SummarySource::User);
    assert!(!confirmed.unchanged);

    let again = w
        .handle()
        .edit_summary_field(
            op,
            edit(SummaryField::Problem, "disk full", 2),
            Actor::User(w.user),
        )
        .await
        .unwrap();
    assert!(again.unchanged);
    assert_eq!(again.revision.revision, 2);
    let revisions = w.store.revisions(op).await.unwrap();
    assert_eq!(
        revisions
            .iter()
            .filter(|r| r.field == SummaryField::Problem)
            .count(),
        2
    );
}

#[tokio::test]
async fn edit_text_is_validated() {
    let w = world().await;
    let op = drafted_failure(&w).await;
    let try_edit = |text: String| {
        let w = &w;
        async move {
            w.handle()
                .edit_summary_field(op, edit(SummaryField::Fix, &text, 1), Actor::User(w.user))
                .await
        }
    };

    assert!(matches!(
        try_edit("   ".to_owned()).await,
        Err(EditSummaryError::Summary(SummaryError::Empty))
    ));
    assert!(matches!(
        try_edit("x".repeat(1001)).await,
        Err(EditSummaryError::Summary(SummaryError::TooLong {
            max: 1000
        }))
    ));
    assert!(matches!(
        try_edit("a\u{7}b".to_owned()).await,
        Err(EditSummaryError::Summary(SummaryError::BadText))
    ));
    assert!(try_edit("x".repeat(1000)).await.is_ok());
}

async fn pending_drafts(w: &World, op: OperationId) -> usize {
    w.effects(op, EffectStatus::Pending)
        .await
        .into_iter()
        .filter(|e| e.kind == EffectKind::DraftResolution.as_str())
        .count()
}

#[tokio::test]
async fn the_backfill_does_not_queue_a_second_draft_for_an_operation_already_queued() {
    let mut w = world_gated().await;
    let op = w.create().await;
    w.handle()
        .finish(op, FinishOutcome::Failed, "nothing".to_owned())
        .await
        .unwrap();
    assert_eq!(pending_drafts(&w, op).await, 1);

    w.restart().await;
    w.settle().await;
    assert_eq!(
        pending_drafts(&w, op).await,
        1,
        "the start-up pass queued no second one"
    );
}

#[tokio::test]
async fn a_summary_survives_a_restart_between_the_terminal_commit_and_the_draft() {
    let mut w = world_gated().await;
    let op = w.create().await;
    w.handle()
        .finish(op, FinishOutcome::Failed, "nothing".to_owned())
        .await
        .unwrap();
    assert!(w.store.current(op).await.unwrap().is_empty());

    w.restart().await;
    w.queue.open();

    let current = until_summary(&w, op).await;
    assert_eq!(current.len(), 5);
}

#[tokio::test]
async fn past_success_does_not_change_what_a_new_step_needs() {
    let w = world().await;
    let first = verified_fix(&w).await;
    until_summary(&w, first).await;

    let op = w.create().await;
    let step = w.propose(op).await;
    for _ in 0..50 {
        w.settle().await;
    }

    let snapshot = w.load(op).await;
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Pending
    );
    let approval = snapshot
        .steps
        .iter()
        .find(|r| r.step.kind == StepKind::Approval)
        .unwrap();
    assert_eq!(approval.progress.outcome, StepOutcome::Pending);
    assert!(w.skill_requests().len() <= 1, "only the first run was sent");
}
