//! The operation trace: ordering, waits, redaction, golden fixtures and JSON snapshots.

#[cfg(test)]
#[path = "operation_trace/scenarios.rs"]
mod scenarios;

use std::collections::BTreeSet;
use std::path::PathBuf;

use domovoi_core::ports::Versioned;
use domovoi_core::{
    ApprovalProvenance, BudgetKind, EdgeKind, ExecutionRecord, OperationSnapshot, OperationStatus,
    PauseReason, PlanBinding, StepEdge, StepIntent, StepKind, StepOutcome, StepRecord, WaitKind,
    build_trace,
};
use jiff::SignedDuration;
use serde_json::{Value, json};

use scenarios::{T0, all, host, human, plan_hash};

#[cfg(test)]
fn fixtures_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/src/test/fixtures/operation-trace/"
    ))
}

#[cfg(test)]
fn scenario(name: &str) -> OperationSnapshot {
    all()
        .into_iter()
        .find(|(file, _)| *file == name)
        .map(|(_, snapshot)| snapshot)
        .unwrap()
}

#[cfg(test)]
fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap();
    text.push('\n');
    text
}

#[test]
fn trace_orders_steps_by_ordinal_and_sorts_edges() {
    let mut snapshot = scenario("failed-verification-new-cycle.json");
    let in_order: Vec<_> = snapshot.steps.iter().map(|r| r.step.id).collect();
    snapshot.steps.reverse();
    snapshot.edges.reverse();
    let trace = build_trace(&snapshot, 42);
    assert_eq!(trace.as_of_seq, 42);
    assert_eq!(
        trace.steps.iter().map(|s| s.id).collect::<Vec<_>>(),
        in_order
    );
    let ordinal = |id| in_order.iter().position(|s| *s == id).unwrap();
    let keys: Vec<_> = trace
        .edges
        .iter()
        .map(|e| (ordinal(e.from), ordinal(e.to), e.kind.as_str()))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(keys.len(), 5);
}

#[cfg(test)]
fn pending(snapshot: &OperationSnapshot, kind: StepKind) -> Vec<&StepRecord> {
    snapshot
        .steps
        .iter()
        .filter(|r| r.step.kind == kind)
        .collect()
}

#[test]
fn trace_waiting_for_lists_waits_in_priority_order() {
    // Verification, question and approval all pending, created in the opposite order of
    // their priority.
    let mut snapshot = scenario("question-and-budget-pause.json");
    snapshot.executions.clear();
    for record in &mut snapshot.steps {
        record.progress.outcome = StepOutcome::Succeeded;
    }
    let set_pending = |snapshot: &mut OperationSnapshot, kind: StepKind| {
        let id = pending(snapshot, kind).first().map(|r| r.step.id).unwrap();
        let record = snapshot.steps.iter_mut().find(|r| r.step.id == id).unwrap();
        record.progress.outcome = StepOutcome::Pending;
    };
    assert!(build_trace(&snapshot, 0).operation.waiting_for.is_empty());

    let verified = scenario("verified-success.json");
    let verification = pending(&verified, StepKind::Verification)[0].clone();
    let mut verification = verification;
    verification.step.ordinal = u32::try_from(snapshot.steps.len()).unwrap();
    verification.progress.outcome = StepOutcome::Pending;
    snapshot.steps.push(verification);
    set_pending(&mut snapshot, StepKind::Question);
    assert_eq!(
        build_trace(&snapshot, 0).operation.waiting_for,
        [WaitKind::User, WaitKind::Timer]
    );
    set_pending(&mut snapshot, StepKind::Approval);
    assert_eq!(
        build_trace(&snapshot, 0).operation.waiting_for,
        [WaitKind::Approval, WaitKind::User, WaitKind::Timer]
    );
}

#[test]
fn trace_waiting_for_names_an_approved_execution_waiting_for_its_agent() {
    let mut snapshot = scenario("waiting-approval.json");
    let run = pending(&snapshot, StepKind::Execution)[0].step.id;
    let mut record: ExecutionRecord = snapshot.executions[0].value.clone();
    record
        .authorize(
            human(),
            ApprovalProvenance::new(None, "web"),
            &PlanBinding::Full(plan_hash(0xab)),
            T0 + SignedDuration::from_secs(5),
        )
        .unwrap();
    record.waiting_since = Some(T0);
    snapshot.executions = vec![Versioned {
        value: record,
        version: 2,
    }];
    assert_eq!(snapshot.executions[0].value.step, run);
    assert_eq!(
        build_trace(&snapshot, 0).operation.waiting_for,
        [WaitKind::Approval, WaitKind::Agent]
    );
}

#[test]
fn trace_summary_is_cut_at_280_characters_on_a_character_boundary() {
    let mut snapshot = scenario("verified-success.json");
    snapshot.steps[0].progress.summary = Some("é".repeat(400));
    let trace = build_trace(&snapshot, 0);
    let summary = trace.steps[0].summary.as_deref().unwrap();
    assert_eq!(summary.chars().count(), 280);
    assert_eq!(summary, "é".repeat(280));
}

#[test]
fn trace_omits_output_intent_and_hashes() {
    for (name, snapshot) in all() {
        let text = serde_json::to_string(&build_trace(&snapshot, 1)).unwrap();
        assert!(!text.contains("RAW-OUTPUT-MARKER"), "{name} leaks output");
        assert!(
            !text.contains(&plan_hash(0xab).to_string()),
            "{name} leaks a plan hash"
        );
        for key in [
            "\"output\"",
            "\"intent\"",
            "\"plan_hash\"",
            "\"provenance\"",
            "\"approved_by\"",
        ] {
            assert!(!text.contains(key), "{name} has {key}");
        }
        assert!(
            !text.contains("restart_container"),
            "{name} leaks the contract"
        );
    }
}

#[test]
fn trace_fixtures_match_the_builder() {
    let update = std::env::var_os("OPERATION_TRACE_UPDATE").is_some();
    for (name, snapshot) in all() {
        let path = fixtures_dir().join(name);
        let built = serde_json::to_value(build_trace(&snapshot, 100)).unwrap();
        if update {
            std::fs::create_dir_all(fixtures_dir()).unwrap();
            std::fs::write(&path, pretty(&built)).unwrap();
        }
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            built, stored,
            "{name} is out of date: run with OPERATION_TRACE_UPDATE=1"
        );
    }
}

#[test]
fn trace_fixtures_cover_every_kind_outcome_and_edge() {
    let mut kinds = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for (_, snapshot) in all() {
        let trace = build_trace(&snapshot, 0);
        kinds.extend(trace.steps.iter().map(|s| s.kind));
        outcomes.extend(trace.steps.iter().map(|s| s.outcome));
        edges.extend(trace.edges.iter().map(|e| e.kind));
    }
    assert_eq!(kinds, StepKind::ALL.into_iter().collect());
    assert_eq!(outcomes, StepOutcome::ALL.into_iter().collect());
    assert_eq!(edges, EdgeKind::ALL.into_iter().collect());
}

#[test]
fn trace_scenarios_obey_the_edge_rules() {
    for (name, snapshot) in all() {
        for StepEdge { from, to, kind, .. } in &snapshot.edges {
            let from = &snapshot.step(*from).unwrap().step;
            let to = &snapshot.step(*to).unwrap().step;
            assert_eq!(domovoi_core::check_edge(from, to, *kind), Ok(()), "{name}");
        }
        for (index, record) in snapshot.steps.iter().enumerate() {
            assert_eq!(
                usize::try_from(record.step.ordinal).unwrap(),
                index,
                "{name}"
            );
        }
    }
}

#[test]
fn operation_trace_json_snapshot() {
    let snapshot = scenario("waiting-approval.json");
    insta::assert_json_snapshot!(build_trace(&snapshot, 100));
}

#[test]
fn step_intent_json_snapshot() {
    let step = domovoi_core::StepId::from_uuid(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0010,
    ));
    let mut intents: Vec<StepIntent> = all()
        .into_iter()
        .flat_map(|(_, s)| s.steps.into_iter().map(|r| r.step.intent))
        .collect();
    intents.push(StepIntent::PolicyDecision { execution: step });
    let mut seen = BTreeSet::new();
    intents.retain(|i| seen.insert(i.kind()));
    intents.sort_by_key(StepIntent::kind);
    assert_eq!(intents.len(), 11);
    insta::assert_json_snapshot!(intents);
}

#[test]
fn operation_status_json_snapshot() {
    let step = domovoi_core::StepId::from_uuid(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0020,
    ));
    let statuses = vec![
        OperationStatus::Queued,
        OperationStatus::Running,
        OperationStatus::WaitingApproval,
        OperationStatus::WaitingUser,
        OperationStatus::WaitingAgent,
        OperationStatus::WaitingTimer,
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::Cycles,
            },
        },
        OperationStatus::Paused {
            reason: PauseReason::OutcomeUnknown { step },
        },
        OperationStatus::Paused {
            reason: PauseReason::GlobalPause,
        },
        OperationStatus::Paused {
            reason: PauseReason::Error {
                message: "disk full".to_owned(),
            },
        },
        OperationStatus::Succeeded,
        OperationStatus::Failed,
        OperationStatus::Escalated,
        OperationStatus::Cancelled,
    ];
    insta::assert_json_snapshot!(statuses);
}

#[test]
fn trace_enum_names_snapshot() {
    insta::assert_json_snapshot!(json!({
        "step_kinds": StepKind::ALL,
        "step_outcomes": StepOutcome::ALL,
        "edge_kinds": EdgeKind::ALL,
        "wait_kinds": [WaitKind::Approval, WaitKind::User, WaitKind::Agent, WaitKind::Timer],
        "budget_kinds": domovoi_core::BudgetKind::ALL,
        "host": host(),
    }));
}
