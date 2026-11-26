//! Closing an investigation: when nothing is left to do, and the task that notices.

use domovoi_core::{
    EdgeKind, FinishOutcome, OperationKind, OperationSnapshot, OperationStatus, StepKind,
    StepOutcome,
};

use super::plumbing::cut as cut_chars;
use super::turn_output::caused_by;

use super::super::OperationHandle;

/// The statuses of an investigation that may still be closed.
const OPEN: [&str; 5] = [
    "running",
    "waiting_approval",
    "waiting_user",
    "waiting_agent",
    "waiting_timer",
];

/// `Some((outcome, summary))` when the investigation has nothing left to do.
///
/// Only an Investigate operation that is running or waiting is closed, and only when no step
/// is pending or running, the latest reasoning turn is final and the notification that turn
/// caused has succeeded. A proposed execution is an open step until it is final, so an
/// investigation never closes while one waits for a person.
pub(crate) fn close_decision(snapshot: &OperationSnapshot) -> Option<(FinishOutcome, String)> {
    let operation = &snapshot.operation;
    let open_status = matches!(
        operation.status,
        OperationStatus::Running
            | OperationStatus::WaitingApproval
            | OperationStatus::WaitingUser
            | OperationStatus::WaitingAgent
            | OperationStatus::WaitingTimer
    );
    if operation.kind != OperationKind::Investigate || !open_status {
        return None;
    }
    if snapshot.open_steps().next().is_some() {
        return None;
    }
    let turn = snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::ReasoningTurn)
        .max_by_key(|record| record.step.ordinal)?;
    let notified = snapshot.steps.iter().any(|record| {
        record.step.kind == StepKind::Notification
            && record.progress.outcome == StepOutcome::Succeeded
            && snapshot.edges.iter().any(|edge| {
                edge.kind == EdgeKind::CausedBy
                    && edge.from == record.step.id
                    && edge.to == turn.step.id
            })
    });
    if !notified {
        return None;
    }
    let reason = turn.progress.reason.clone().unwrap_or_default();
    // A report that was delivered is not a success when the action it led to failed or may
    // not have run: the operation says so instead of showing success beside a failed step.
    if turn.progress.outcome == StepOutcome::Succeeded
        && let Some(action) = caused_by(snapshot, turn.step.id, StepKind::Execution)
            .into_iter()
            .find(|record| {
                matches!(
                    record.progress.outcome,
                    StepOutcome::Failed | StepOutcome::OutcomeUnknown
                )
            })
    {
        let why = action
            .progress
            .reason
            .clone()
            .unwrap_or_else(|| action.progress.outcome.as_str().to_owned());
        return Some((
            FinishOutcome::Failed,
            cut_chars(&format!("action failed: {why}"), 200),
        ));
    }
    Some(match turn.progress.outcome {
        StepOutcome::Succeeded => (FinishOutcome::Succeeded, "report delivered".to_owned()),
        StepOutcome::NotRun => (
            FinishOutcome::Failed,
            cut_chars(&format!("skipped: {reason}"), 200),
        ),
        _ => (FinishOutcome::Failed, cut_chars(&reason, 200)),
    })
}

/// Closes `operation` when it has nothing left to do. A refusal is ignored: the closer
/// looks again on its next pass.
pub(super) async fn try_close(handle: &OperationHandle, snapshot: &OperationSnapshot) {
    if let Some((outcome, summary)) = close_decision(snapshot) {
        let operation = snapshot.operation.id;
        if let Err(err) = handle.finish(operation, outcome, summary).await {
            tracing::debug!(%err, %operation, "an investigation was not closed yet");
        }
    }
}

/// Looks at every open investigation each `close_interval` until shutdown.
pub(super) async fn run_closer(handle: OperationHandle) {
    let shared = handle.shared.clone();
    let Some(runtime) = shared.investigate.clone() else {
        return;
    };
    let clock = shared.deps.clock.clone();
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let now = clock.now();
        let next = now
            .checked_add(runtime.settings.close_interval)
            .unwrap_or(now);
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = clock.sleep_until(next) => {}
        }
        let open = match shared.deps.operations.with_status(&OPEN, 200).await {
            Ok(open) => open,
            Err(err) => {
                tracing::warn!(%err, "could not list the open investigations");
                continue;
            }
        };
        for operation in open
            .into_iter()
            .filter(|op| op.kind == OperationKind::Investigate)
        {
            match shared.deps.operations.load(operation.id).await {
                Ok(Some(snapshot)) => try_close(&handle, &snapshot).await,
                Ok(None) => {}
                Err(err) => tracing::warn!(%err, "could not read an open investigation"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{
        Budgets, CheckKey, EdgeKind, HostId, Operation, OperationId, PinnedSubject, Step, StepEdge,
        StepId, StepIntent, StepProgress, StepRecord,
    };
    use jiff::Timestamp;

    use super::*;

    fn subject() -> PinnedSubject {
        PinnedSubject {
            host: HostId::new(),
            check: CheckKey::parse("disk.usage").expect("a check key"),
            target: None,
        }
    }

    fn snapshot(kind: OperationKind, status: OperationStatus) -> OperationSnapshot {
        let mut operation = Operation::queued(
            OperationId::new(),
            kind,
            "Investigate: disk",
            vec![subject()],
            None,
            Budgets::new(Default::default()),
            Timestamp::UNIX_EPOCH,
        )
        .expect("an operation");
        operation.status = status;
        OperationSnapshot {
            operation,
            version: 1,
            steps: Vec::new(),
            edges: Vec::new(),
            executions: Vec::new(),
            incidents: Vec::new(),
        }
    }

    fn add(snap: &mut OperationSnapshot, intent: StepIntent, outcome: StepOutcome) -> StepId {
        let id = StepId::new();
        let ordinal = u32::try_from(snap.steps.len()).expect("few steps");
        snap.steps.push(StepRecord {
            step: Step {
                id,
                operation: snap.operation.id,
                ordinal,
                kind: intent.kind(),
                phase: "p".to_owned(),
                parent: None,
                cycle: 0,
                intent,
                created_at: Timestamp::UNIX_EPOCH,
            },
            progress: StepProgress {
                outcome,
                reason: Some("because".to_owned()),
                ..StepProgress::pending()
            },
        });
        id
    }

    fn turn(snap: &mut OperationSnapshot, outcome: StepOutcome) -> StepId {
        add(
            snap,
            StepIntent::ReasoningTurn {
                context_refs: Vec::new(),
            },
            outcome,
        )
    }

    fn notification(snap: &mut OperationSnapshot, turn: StepId, outcome: StepOutcome) {
        let id = add(
            snap,
            StepIntent::Notification {
                channel: "incident".to_owned(),
                subject: "s".to_owned(),
            },
            outcome,
        );
        snap.edges.push(StepEdge {
            operation: snap.operation.id,
            from: id,
            to: turn,
            kind: EdgeKind::CausedBy,
        });
    }

    fn running() -> OperationSnapshot {
        snapshot(OperationKind::Investigate, OperationStatus::Running)
    }

    #[test]
    fn nothing_closes_without_a_turn_or_with_a_running_one() {
        assert_eq!(close_decision(&running()), None);
        let mut snap = running();
        turn(&mut snap, StepOutcome::Running);
        assert_eq!(close_decision(&snap), None);
    }

    #[test]
    fn a_finished_turn_without_a_succeeded_notification_stays_open() {
        let mut snap = running();
        let t = turn(&mut snap, StepOutcome::Succeeded);
        assert_eq!(close_decision(&snap), None);
        notification(&mut snap, t, StepOutcome::Pending);
        assert_eq!(close_decision(&snap), None);
    }

    #[test]
    fn a_delivered_report_closes_succeeded() {
        let mut snap = running();
        let t = turn(&mut snap, StepOutcome::Succeeded);
        notification(&mut snap, t, StepOutcome::Succeeded);
        let (outcome, summary) = close_decision(&snap).expect("it closes");
        assert_eq!(outcome, FinishOutcome::Succeeded);
        assert_eq!(summary, "report delivered");
    }

    #[test]
    fn failed_and_skipped_turns_close_failed_with_their_reason() {
        for (outcome, text) in [
            (StepOutcome::Failed, "because"),
            (StepOutcome::NotRun, "skipped: because"),
        ] {
            let mut snap = running();
            let t = turn(&mut snap, outcome);
            notification(&mut snap, t, StepOutcome::Succeeded);
            assert_eq!(
                close_decision(&snap),
                Some((FinishOutcome::Failed, text.to_owned()))
            );
        }
    }

    #[test]
    fn a_failed_or_unknown_action_closes_the_investigation_failed() {
        for outcome in [StepOutcome::Failed, StepOutcome::OutcomeUnknown] {
            let mut snap = running();
            let t = turn(&mut snap, StepOutcome::Succeeded);
            notification(&mut snap, t, StepOutcome::Succeeded);
            let action = add(
                &mut snap,
                StepIntent::Assessment {
                    text: "restart".to_owned(),
                },
                outcome,
            );
            // The test intent is a stand-in: the step is stored as a child of the turn.
            let record = snap.steps.iter_mut().find(|r| r.step.id == action).unwrap();
            record.step.kind = StepKind::Execution;
            record.step.parent = Some(t);
            let (finish, summary) = close_decision(&snap).expect("it closes");
            assert_eq!(finish, FinishOutcome::Failed);
            assert_eq!(summary, "action failed: because");
        }
    }

    #[test]
    fn a_denied_action_does_not_fail_the_investigation() {
        let mut snap = running();
        let t = turn(&mut snap, StepOutcome::Succeeded);
        notification(&mut snap, t, StepOutcome::Succeeded);
        let action = add(
            &mut snap,
            StepIntent::Assessment {
                text: "restart".to_owned(),
            },
            StepOutcome::Refused,
        );
        let record = snap.steps.iter_mut().find(|r| r.step.id == action).unwrap();
        record.step.kind = StepKind::Execution;
        record.step.parent = Some(t);
        assert_eq!(
            close_decision(&snap),
            Some((FinishOutcome::Succeeded, "report delivered".to_owned()))
        );
    }

    #[test]
    fn a_pending_execution_keeps_the_investigation_open() {
        let mut snap = running();
        let t = turn(&mut snap, StepOutcome::Succeeded);
        notification(&mut snap, t, StepOutcome::Succeeded);
        add(
            &mut snap,
            StepIntent::Assessment {
                text: "a".to_owned(),
            },
            StepOutcome::Pending,
        );
        assert_eq!(close_decision(&snap), None);
    }

    #[test]
    fn only_a_running_or_waiting_investigation_is_closed() {
        for status in [
            OperationStatus::Queued,
            OperationStatus::Succeeded,
            OperationStatus::Cancelled,
        ] {
            let mut snap = snapshot(OperationKind::Investigate, status);
            let t = turn(&mut snap, StepOutcome::Succeeded);
            notification(&mut snap, t, StepOutcome::Succeeded);
            assert_eq!(close_decision(&snap), None);
        }
        let mut waiting = snapshot(OperationKind::Investigate, OperationStatus::WaitingApproval);
        let t = turn(&mut waiting, StepOutcome::Succeeded);
        notification(&mut waiting, t, StepOutcome::Succeeded);
        assert!(close_decision(&waiting).is_some());
    }

    #[test]
    fn the_closer_leaves_an_investigation_with_a_pending_question_open() {
        let mut snap = running();
        let asking = turn(&mut snap, StepOutcome::Succeeded);
        let question = add(
            &mut snap,
            StepIntent::Question {
                text: "which one?".to_owned(),
                expires_at: Timestamp::UNIX_EPOCH,
            },
            StepOutcome::Pending,
        );
        snap.edges.push(StepEdge {
            operation: snap.operation.id,
            from: question,
            to: asking,
            kind: EdgeKind::CausedBy,
        });
        assert_eq!(close_decision(&snap), None, "the question is still open");
        // Answered: the asking turn is the newest and has no notification, so it stays open
        // until the resumed turn's notification has succeeded.
        for record in &mut snap.steps {
            if record.step.id == question {
                record.progress.outcome = StepOutcome::Succeeded;
            }
        }
        assert_eq!(close_decision(&snap), None);
        let resumed = turn(&mut snap, StepOutcome::Succeeded);
        assert_eq!(close_decision(&snap), None);
        notification(&mut snap, resumed, StepOutcome::Succeeded);
        assert!(close_decision(&snap).is_some());
    }
}
