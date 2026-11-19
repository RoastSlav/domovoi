//! Whatever events arrive, in whatever order, an operation keeps its invariants.

use domovoi_core::ports::JournalKind;
use domovoi_core::{
    BudgetKind, BudgetLimits, ExecutionId, FinishOutcome, OperationSnapshot, OperationStatus,
    PauseReason, StepId, StepKind, StepOutcome, build_trace, check_edge,
};
use domovoi_engine::operations::decide::decide;
use domovoi_engine::operations::{OperationEvent, OperationState, derive_status};
use jiff::SignedDuration;
use proptest::prelude::*;

use crate::support::Fixture;

/// Text that must never reach a journal or audit entry.
const MARKER: &str = "OUTPUT-MARKER-7f3a";

#[derive(Debug, Clone)]
enum Action {
    Admit,
    Evidence,
    Plan,
    Question,
    Turn,
    Run(u8),
    Authorize(usize),
    Deny(usize),
    Expire(usize),
    WaitForAgent(usize),
    Dispatch(usize),
    Settle(usize, u8, bool),
    Verified(usize, bool),
    Start(usize),
    Finish(usize, u8),
    Answer(usize),
    Cancel,
    Pause,
    Resume(bool),
    End(u8),
    Tick,
    Advance(i64),
    /// A whole run, from proposal to verification, aimed at the newest steps.
    Flow(u8, u8, bool),
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        1 => Just(Action::Admit),
        3 => Just(Action::Evidence),
        1 => Just(Action::Plan),
        2 => Just(Action::Question),
        1 => Just(Action::Turn),
        4 => (0u8..3).prop_map(Action::Run),
        4 => any::<usize>().prop_map(Action::Authorize),
        1 => any::<usize>().prop_map(Action::Deny),
        1 => any::<usize>().prop_map(Action::Expire),
        1 => any::<usize>().prop_map(Action::WaitForAgent),
        4 => any::<usize>().prop_map(Action::Dispatch),
        4 => (any::<usize>(), 0u8..5, any::<bool>()).prop_map(|(i, o, v)| Action::Settle(i, o, v)),
        3 => (any::<usize>(), any::<bool>()).prop_map(|(i, p)| Action::Verified(i, p)),
        2 => any::<usize>().prop_map(Action::Start),
        2 => (any::<usize>(), 0u8..4).prop_map(|(i, o)| Action::Finish(i, o)),
        1 => any::<usize>().prop_map(Action::Answer),
        1 => Just(Action::Cancel),
        1 => Just(Action::Pause),
        2 => any::<bool>().prop_map(Action::Resume),
        1 => (0u8..3).prop_map(Action::End),
        1 => Just(Action::Tick),
        2 => (1i64..90_000).prop_map(Action::Advance),
        6 => (0u8..3, 0u8..4, any::<bool>()).prop_map(|(v, o, p)| Action::Flow(v, o, p)),
    ]
}

fn limits(preset: u8) -> BudgetLimits {
    match preset % 5 {
        0 => BudgetLimits::default(),
        1 => BudgetLimits {
            evidence_requests: Some(2),
            executions: Some(2),
            cycles: Some(1),
            ..BudgetLimits::default()
        },
        2 => BudgetLimits {
            executions: Some(1),
            wall_seconds: Some(3600),
            ..BudgetLimits::default()
        },
        3 => BudgetLimits {
            model_turns: Some(1),
            tokens: Some(50),
            ..BudgetLimits::default()
        },
        _ => BudgetLimits {
            evidence_requests: Some(1),
            cycles: Some(2),
            ..BudgetLimits::default()
        },
    }
}

/// The ids of steps of `kind` in one of `outcomes`.
fn pick(
    state: &OperationState,
    index: usize,
    kind: StepKind,
    outcomes: &[StepOutcome],
) -> Option<StepId> {
    let matching: Vec<StepId> = state
        .snapshot()
        .steps
        .iter()
        .filter(|r| r.step.kind == kind && outcomes.contains(&r.progress.outcome))
        .map(|r| r.step.id)
        .collect();
    let last = matching.len().checked_sub(1)?;
    Some(
        matching[if index == usize::MAX {
            last
        } else {
            index % matching.len()
        }],
    )
}

const OPEN: [StepOutcome; 2] = [StepOutcome::Pending, StepOutcome::Running];

/// The event an action stands for in the current state, if any.
fn event_for(fx: &mut Fixture, action: &Action) -> Option<OperationEvent> {
    let state = fx.state.clone();
    let settle_outcomes = [
        StepOutcome::Succeeded,
        StepOutcome::Failed,
        StepOutcome::OutcomeUnknown,
        StepOutcome::NotRun,
        StepOutcome::Refused,
    ];
    Some(match action {
        Action::Admit => OperationEvent::Admit,
        Action::Evidence => {
            let step = fx.evidence();
            OperationEvent::Propose {
                steps: vec![step],
                edges: Vec::new(),
            }
        }
        Action::Plan => {
            let step = fx.plan();
            OperationEvent::Propose {
                steps: vec![step],
                edges: Vec::new(),
            }
        }
        Action::Question => {
            let step = fx.question(3600);
            OperationEvent::Propose {
                steps: vec![step],
                edges: Vec::new(),
            }
        }
        Action::Turn => {
            let step = fx.new_step(domovoi_core::StepIntent::ReasoningTurn {
                context_refs: Vec::new(),
            });
            OperationEvent::Propose {
                steps: vec![step],
                edges: Vec::new(),
            }
        }
        Action::Run(variant) => {
            let intent = fx.intent(&[("v", &variant.to_string())]);
            OperationEvent::ProposeExecution(fx.proposal(intent))
        }
        Action::Authorize(i) => {
            let step = pick(&state, *i, StepKind::Execution, &[StepOutcome::Pending])?;
            OperationEvent::ExecutionAuthorized { step }
        }
        Action::Deny(i) => OperationEvent::ExecutionDenied {
            step: pick(&state, *i, StepKind::Execution, &[StepOutcome::Pending])?,
        },
        Action::Expire(i) => OperationEvent::ExecutionExpired {
            step: pick(&state, *i, StepKind::Execution, &[StepOutcome::Pending])?,
        },
        Action::WaitForAgent(i) => OperationEvent::WaitingForAgent {
            step: pick(&state, *i, StepKind::Execution, &[StepOutcome::Pending])?,
        },
        Action::Dispatch(i) => OperationEvent::ExecutionDispatched {
            step: pick(&state, *i, StepKind::Execution, &[StepOutcome::Pending])?,
            epoch: domovoi_core::JournalEpoch::from_uuid(uuid::Uuid::from_u128(9)),
            reconcile_after: fx.now + SignedDuration::from_secs(60),
        },
        Action::Settle(i, o, verify) => OperationEvent::ExecutionSettled {
            step: pick(&state, *i, StepKind::Execution, &OPEN)?,
            outcome: if *o == 4 {
                StepOutcome::Unavailable
            } else {
                settle_outcomes[usize::from(*o) % settle_outcomes.len()]
            },
            reason: Some("reason".to_owned()),
            output: Some(MARKER.to_owned()),
            verify_after: verify.then(|| SignedDuration::from_secs(30)),
        },
        Action::Verified(i, passed) => OperationEvent::Verified {
            step: pick(&state, *i, StepKind::Verification, &OPEN)?,
            passed: *passed,
            summary: "checked".to_owned(),
        },
        Action::Start(i) => {
            let kinds = [
                StepKind::EvidenceRequest,
                StepKind::Notification,
                StepKind::Verification,
            ];
            let step = kinds
                .iter()
                .find_map(|k| pick(&state, *i, *k, &[StepOutcome::Pending]))?;
            OperationEvent::StepStarted { step }
        }
        Action::Finish(i, o) => {
            let kinds = [
                StepKind::EvidenceRequest,
                StepKind::ReasoningTurn,
                StepKind::Question,
            ];
            let step = kinds.iter().find_map(|k| pick(&state, *i, *k, &OPEN))?;
            let outcomes = [
                StepOutcome::Succeeded,
                StepOutcome::Failed,
                StepOutcome::Unavailable,
                StepOutcome::NotRun,
            ];
            OperationEvent::StepFinished {
                step,
                outcome: outcomes[usize::from(*o) % outcomes.len()],
                summary: Some("summary".to_owned()),
                reason: None,
                output: Some(MARKER.to_owned()),
                usage: Some(domovoi_core::StepUsage {
                    input_tokens: 20,
                    output_tokens: 10,
                    cost_micros: Some(5),
                }),
            }
        }
        Action::Answer(i) => {
            let question = pick(&state, *i, StepKind::Question, &[StepOutcome::Pending])?;
            let note = fx.note();
            OperationEvent::Answer { question, note }
        }
        Action::Cancel => OperationEvent::Cancel {
            reason: "stop".to_owned(),
        },
        Action::Pause => OperationEvent::Pause {
            reason: PauseReason::GlobalPause,
        },
        Action::Resume(extend) => OperationEvent::Resume {
            extend: if *extend {
                BudgetKind::ALL.iter().map(|k| (*k, None)).collect()
            } else {
                Vec::new()
            },
        },
        Action::End(o) => OperationEvent::Finish {
            outcome: [
                FinishOutcome::Succeeded,
                FinishOutcome::Failed,
                FinishOutcome::Escalated,
            ][usize::from(*o) % 3],
            summary: "done".to_owned(),
        },
        Action::Tick => OperationEvent::Tick,
        Action::Advance(secs) => {
            fx.advance(*secs);
            return None;
        }
        Action::Flow(..) => return None,
    })
}

/// Runs one action: the way a writer would, approving the row first when the event is an
/// authorization and stamping `waiting_since` after an agent wait, and discarding the lot
/// when `decide` refuses.
fn step(fx: &mut Fixture, action: &Action) {
    if let Action::Flow(variant, outcome, passed) = action {
        let last = usize::MAX;
        for part in [
            Action::Run(*variant),
            Action::Authorize(last),
            Action::Dispatch(last),
            Action::Settle(last, *outcome, true),
            Action::Verified(last, *passed),
        ] {
            step(fx, &part);
        }
        return;
    }
    let Some(event) = event_for(fx, action) else {
        return;
    };
    let saved = fx.state.clone();
    if let OperationEvent::ExecutionAuthorized { step } = &event {
        let row = fx.state.execution_of(*step);
        if !row.is_some_and(|r| r.approval().is_none() && fx.now < r.expires_at) {
            return;
        }
        fx.approve_row(*step);
    }
    let before = fx.state.clone();
    let mut first_ids = fx.ids.clone();
    let mut second_ids = fx.ids.clone();
    let first = decide(&before, event.clone(), fx.now, &mut first_ids);
    let second = decide(&before, event.clone(), fx.now, &mut second_ids);
    assert_eq!(first, second, "decide is deterministic");
    assert_eq!(before, fx.state, "decide leaves its input alone");
    let Ok(decision) = first else {
        fx.state = saved;
        return;
    };
    fx.ids = first_ids;
    let was_terminal = before.operation().status.is_terminal();
    let was = before.operation().status.clone();
    fx.state = before.apply(&decision);
    if let OperationEvent::WaitingForAgent { step } = &event {
        let now = fx.now;
        fx.tweak(|s| {
            if let Some(row) = s.executions.iter_mut().find(|r| r.value.step == *step) {
                row.value.waiting_since.get_or_insert(now);
            }
        });
    }
    if was_terminal {
        assert_eq!(
            fx.state.operation().status,
            was,
            "a finished operation stays finished"
        );
    }
    assert_eq!(decision.operation.updated_at, fx.now);
    check_journal(&decision);
    check_state(fx.state.snapshot());
}

/// I6 and I9: every change has a journal entry, and no output reaches journal or audit.
fn check_journal(decision: &domovoi_engine::operations::Decision) {
    let notes = |kind: JournalKind, step: StepId| {
        decision
            .journal
            .iter()
            .filter(|n| n.kind == kind && n.payload["step"] == step.to_string())
            .count()
    };
    for record in &decision.steps {
        assert_eq!(notes(JournalKind::StepCreated, record.step.id), 1);
        match record.progress.outcome {
            StepOutcome::Pending => {}
            StepOutcome::Running => assert_eq!(notes(JournalKind::StepStarted, record.step.id), 1),
            _ => assert_eq!(notes(JournalKind::StepFinished, record.step.id), 1),
        }
    }
    for change in &decision.progress {
        let kind = if change.to.outcome == StepOutcome::Running {
            JournalKind::StepStarted
        } else {
            JournalKind::StepFinished
        };
        assert!(notes(kind, change.step) >= 1, "{change:?}");
    }
    for note in &decision.journal {
        assert!(
            !note.payload.to_string().contains(MARKER),
            "output in the journal"
        );
    }
    for note in &decision.audit {
        assert!(
            !note.detail.to_string().contains(MARKER),
            "output in the audit log"
        );
        assert_eq!(note.detail["operation"].as_str().map(str::len), Some(36));
        assert!(note.detail.get("step").is_some());
    }
}

/// I1 to I5 and I7.
fn check_state(snapshot: &OperationSnapshot) {
    let op = &snapshot.operation;
    for (i, record) in snapshot.steps.iter().enumerate() {
        assert_eq!(record.step.operation, op.id);
        assert_eq!(
            usize::try_from(record.step.ordinal).unwrap(),
            i,
            "dense ordinals"
        );
        if let Some(parent) = record.step.parent {
            let parent = snapshot.step(parent).expect("parent exists");
            assert!(parent.step.ordinal < record.step.ordinal);
        }
        let row = snapshot.execution_of(record.step.id);
        if record.step.kind == StepKind::Execution {
            let refused_without_row = record.progress.outcome == StepOutcome::Refused;
            assert!(
                row.is_some() || refused_without_row,
                "an execution has a row: {record:?}"
            );
        } else {
            assert!(row.is_none(), "only executions have rows");
        }
    }
    for edge in &snapshot.edges {
        assert_eq!(edge.operation, op.id);
        let (from, to) = (
            snapshot.step(edge.from).unwrap(),
            snapshot.step(edge.to).unwrap(),
        );
        check_edge(&from.step, &to.step, edge.kind).unwrap();
    }
    let unique: std::collections::HashSet<_> = snapshot
        .edges
        .iter()
        .map(|e| (e.from, e.to, e.kind))
        .collect();
    assert_eq!(unique.len(), snapshot.edges.len(), "no duplicate edges");
    for row in &snapshot.executions {
        assert_eq!(row.value.operation, op.id);
        assert_eq!(row.value.execution, ExecutionId::from_step(row.value.step));
    }
    if matches!(
        op.status,
        OperationStatus::Running
            | OperationStatus::WaitingApproval
            | OperationStatus::WaitingUser
            | OperationStatus::WaitingAgent
            | OperationStatus::WaitingTimer
    ) {
        assert_eq!(
            op.status,
            derive_status(snapshot),
            "stored status is the derived one"
        );
    }
    if op.status.is_terminal() {
        assert!(op.ended_at.is_some());
        for record in snapshot.open_steps() {
            let dispatched = record.step.kind == StepKind::Execution
                && record.progress.outcome == StepOutcome::Running;
            assert!(dispatched, "open step on a finished operation: {record:?}");
        }
    }
    let (limits, used) = (&op.budgets.limits, &op.budgets.used);
    assert!(
        limits
            .evidence_requests
            .is_none_or(|l| used.evidence_requests <= l)
    );
    assert!(limits.executions.is_none_or(|l| used.executions <= l));
    if let Some(reason) = op.status.pause_reason()
        && let PauseReason::Budget { exhausted } = reason
    {
        assert!(
            op.budgets.is_spent(*exhausted, op.cycle),
            "paused for a budget with room"
        );
    }
    let trace = build_trace(snapshot, 0);
    assert_eq!(trace.steps.len(), snapshot.steps.len());
    assert_eq!(trace.edges.len(), snapshot.edges.len());
    assert!(!serde_json::to_string(&trace).unwrap().contains(MARKER));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn decide_never_breaks_trace_invariants(
        preset in any::<u8>(),
        admitted in any::<bool>(),
        actions in proptest::collection::vec(action(), 1..40),
    ) {
        let mut fx = if admitted {
            Fixture::running(limits(preset))
        } else {
            Fixture::new(limits(preset))
        };
        for action in &actions {
            step(&mut fx, action);
        }
    }
}
